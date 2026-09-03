//! SQL-backed policy reader (plans/m6-privacy.md §6, §70). Versioned legal
//! pages per locale (§102): `current` (latest effective, not superseded) +
//! `history` (all). Locale fallback (→ pt-BR) is the caller's job.

use crate::Db;
use async_trait::async_trait;
use bikenest_application::{PolicyDocument, PolicyReader, PrivacyError};
use bikenest_domain::PolicyKind;
use chrono::{DateTime, Utc};

pub struct SqlxPolicyReader {
    db: Db,
}

impl SqlxPolicyReader {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[derive(sqlx::FromRow)]
struct PolicyRow {
    id: i64,
    kind: String,
    locale: String,
    version: String,
    effective_at: DateTime<Utc>,
    superseded_at: Option<DateTime<Utc>>,
    content: String,
}

impl PolicyRow {
    fn into_document(self) -> Result<PolicyDocument, PrivacyError> {
        let kind = PolicyKind::from_code(&self.kind).map_err(|_| PrivacyError::Internal)?;
        Ok(PolicyDocument {
            id: self.id,
            kind,
            locale: self.locale,
            version: self.version,
            effective_at: self.effective_at,
            superseded_at: self.superseded_at,
            content: self.content,
        })
    }
}

#[async_trait]
impl PolicyReader for SqlxPolicyReader {
    async fn current(
        &self,
        kind: PolicyKind,
        locale: &str,
    ) -> Result<Option<PolicyDocument>, PrivacyError> {
        let row = sqlx::query_as::<_, PolicyRow>(
            r#"
            SELECT id, kind, locale, version, effective_at, superseded_at, content
            FROM policy_version
            WHERE kind = $1 AND locale = $2 AND superseded_at IS NULL
            ORDER BY effective_at DESC
            LIMIT 1
            "#,
        )
        .bind(kind.as_code())
        .bind(locale)
        .fetch_optional(self.db.pool())
        .await
        .map_err(map_err)?;
        match row {
            Some(r) => Ok(Some(r.into_document()?)),
            None => Ok(None),
        }
    }

    async fn history(
        &self,
        kind: PolicyKind,
        locale: &str,
    ) -> Result<Vec<PolicyDocument>, PrivacyError> {
        let rows = sqlx::query_as::<_, PolicyRow>(
            r#"
            SELECT id, kind, locale, version, effective_at, superseded_at, content
            FROM policy_version
            WHERE kind = $1 AND locale = $2
            ORDER BY effective_at DESC, id DESC
            "#,
        )
        .bind(kind.as_code())
        .bind(locale)
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;
        rows.into_iter().map(PolicyRow::into_document).collect()
    }
}

fn map_err(_e: sqlx::Error) -> PrivacyError {
    PrivacyError::Internal
}
