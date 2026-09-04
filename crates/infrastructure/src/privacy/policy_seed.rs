//! Policy seeding for the versioned legal pages (plans/m6-privacy.md §8, §70/§71).
//!
//! `seed-policies` reads `policies/{kind}.{locale}.md` and upserts one
//! `policy_version` row per `(kind, locale)`, **keyed on `(kind, locale,
//! version)`** — re-running with the same version string is a no-op
//! (idempotent), and a *new* version supersedes the current one for that
//! locale.
//!
//! The markdown carries `{{TOKEN}}` placeholders for the operator's identity
//! and contact channel (see [`POLICY_PLACEHOLDERS`]) so the company details
//! live in the deployment environment, not in the repository. Seeding refuses
//! to publish a document with an unresolved placeholder.

use crate::Db;
use bikenest_application::PrivacyError;
use bikenest_domain::PolicyKind;
use chrono::{DateTime, Utc};

/// Locales a policy document is published in (`policy_version.locale`).
/// pt-BR is the fallback the web layer uses when a locale has no document.
pub const POLICY_LOCALES: &[&str] = &["pt-BR", "en"];

/// `{{TOKEN}}` → environment variable that supplies it at seed time (§70:
/// controller identity + contact information).
pub const POLICY_PLACEHOLDERS: &[(&str, &str)] = &[
    ("OPERATOR_NAME", "POLICY_OPERATOR_NAME"),
    ("OPERATOR_CNPJ", "POLICY_OPERATOR_CNPJ"),
    ("OPERATOR_ADDRESS", "POLICY_OPERATOR_ADDRESS"),
    ("CONTACT_EMAIL", "POLICY_CONTACT_EMAIL"),
];

/// Replace every `{{TOKEN}}` in `content` using `lookup`. Tokens are
/// `[A-Z0-9_]+`; anything else between double braces is left untouched.
/// Returns the distinct unresolved token names when any lookup fails, so the
/// caller can refuse to seed legal text with holes in it.
pub fn fill_policy_placeholders(
    content: &str,
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<String, Vec<String>> {
    let mut out = String::with_capacity(content.len());
    let mut missing: Vec<String> = Vec::new();
    let mut rest = content;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let inner = &after[..end];
        let token = inner.trim();
        if is_token(token) {
            match lookup(token) {
                Some(value) => out.push_str(&value),
                None => {
                    if !missing.iter().any(|m| m == token) {
                        missing.push(token.to_string());
                    }
                }
            }
        } else {
            out.push_str("{{");
            out.push_str(inner);
            out.push_str("}}");
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    if missing.is_empty() {
        Ok(out)
    } else {
        Err(missing)
    }
}

fn is_token(t: &str) -> bool {
    !t.is_empty()
        && t.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Install one `policy_version` row. Idempotent by `(kind, locale, version)`:
/// - same `version` already present → no-op.
/// - a *new* `version` → supersede the current row for that locale and insert.
pub async fn seed_policy(
    db: &Db,
    kind: PolicyKind,
    locale: &str,
    version: &str,
    effective_at: DateTime<Utc>,
    content: &str,
) -> Result<(), PrivacyError> {
    // Idempotency: a row with this (kind, locale, version) already exists → done.
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM policy_version WHERE kind = $1 AND locale = $2 AND version = $3)",
    )
    .bind(kind.as_code())
    .bind(locale)
    .bind(version)
    .fetch_one(db.pool())
    .await
    .map_err(|e| db_err("policy_seed.seed_policy", e))?;
    if exists {
        return Ok(());
    }

    // Supersede the current row, but only when the incoming version is actually
    // newer (no effective-date conflict — an older version must not dethrone a
    // newer current one).
    sqlx::query(
        "UPDATE policy_version SET superseded_at = $3 \
         WHERE kind = $1 AND locale = $2 AND superseded_at IS NULL AND effective_at < $3",
    )
    .bind(kind.as_code())
    .bind(locale)
    .bind(effective_at)
    .execute(db.pool())
    .await
    .map_err(|e| db_err("policy_seed.seed_policy", e))?;

    // Insert the new current version.
    sqlx::query(
        r#"
        INSERT INTO policy_version (kind, locale, version, effective_at, content)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(kind.as_code())
    .bind(locale)
    .bind(version)
    .bind(effective_at)
    .bind(content)
    .execute(db.pool())
    .await
    .map_err(|e| db_err("policy_seed.seed_policy", e))?;

    Ok(())
}

/// Classify + log the sqlx error (SQLSTATE, constraint), then map it onto
/// the feature error. `context` names the operation, e.g. `"policy_seed.insert"`.
fn db_err(context: &'static str, e: sqlx::Error) -> PrivacyError {
    crate::db_error::classify_and_log(context, e).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(token: &str) -> Option<String> {
        match token {
            "OPERATOR_NAME" => Some("Acme Ltda.".to_string()),
            "CONTACT_EMAIL" => Some("privacidade@example.com".to_string()),
            _ => None,
        }
    }

    #[test]
    fn fills_known_tokens_and_leaves_other_braces_alone() {
        let out = fill_policy_placeholders(
            "Operated by {{OPERATOR_NAME}} ({{ CONTACT_EMAIL }}). Not a token: {{not one}} {{",
            lookup,
        )
        .unwrap();
        assert_eq!(
            out,
            "Operated by Acme Ltda. (privacidade@example.com). Not a token: {{not one}} {{"
        );
    }

    #[test]
    fn reports_each_missing_token_once() {
        let err = fill_policy_placeholders(
            "{{OPERATOR_CNPJ}} {{OPERATOR_NAME}} {{OPERATOR_CNPJ}} {{OPERATOR_ADDRESS}}",
            lookup,
        )
        .unwrap_err();
        assert_eq!(
            err,
            vec!["OPERATOR_CNPJ".to_string(), "OPERATOR_ADDRESS".to_string()]
        );
    }

    #[test]
    fn every_placeholder_has_an_env_var() {
        for (token, var) in POLICY_PLACEHOLDERS {
            assert!(is_token(token), "{token} must be an uppercase token");
            assert!(var.starts_with("POLICY_"), "{var} must be namespaced");
        }
    }
}
