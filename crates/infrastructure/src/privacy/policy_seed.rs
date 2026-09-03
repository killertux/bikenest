//! Policy seeding for the versioned legal pages (plans/m6-privacy.md §8, §71).
//!
//! `seed-policies` reads `policies/*.md` and upserts a new `policy_version` row
//! per kind, **keyed on `(kind, version)`** — so re-running with the same
//! version string is a no-op (idempotent), and a *new* version supersedes the
//! current one. The content is placeholder legal text (§71), never treated as
//! final.

use crate::Db;
use bikenest_application::PrivacyError;
use bikenest_domain::PolicyKind;
use chrono::{DateTime, Utc};

/// Install one `policy_version` row. Idempotent by `(kind, version)`:
/// - same `version` already present → no-op.
/// - a *new* `version` → supersede the current row and insert the new one.
pub async fn seed_policy(
    db: &Db,
    kind: PolicyKind,
    version: &str,
    effective_at: DateTime<Utc>,
    content: &str,
) -> Result<(), PrivacyError> {
    // Idempotency: a row with this (kind, version) already exists → done.
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM policy_version WHERE kind = $1 AND version = $2)",
    )
    .bind(kind.as_code())
    .bind(version)
    .fetch_one(db.pool())
    .await
    .map_err(map_err)?;
    if exists {
        return Ok(());
    }

    // Supersede the current row, but only when the incoming version is actually
    // newer (no effective-date conflict — an older version must not dethrone a
    // newer current one).
    sqlx::query("UPDATE policy_version SET superseded_at = $2 \
         WHERE kind = $1 AND superseded_at IS NULL AND effective_at < $2").bind(kind.as_code()).bind(effective_at)
    .execute(db.pool())
    .await
    .map_err(map_err)?;

    // Insert the new current version.
    sqlx::query(r#"
        INSERT INTO policy_version (kind, version, effective_at, content)
        VALUES ($1, $2, $3, $4)
        "#).bind(kind.as_code()).bind(version).bind(effective_at).bind(content)
    .execute(db.pool())
    .await
    .map_err(map_err)?;

    Ok(())
}

fn map_err(_e: sqlx::Error) -> PrivacyError {
    PrivacyError::Internal
}
