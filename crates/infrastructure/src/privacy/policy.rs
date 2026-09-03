//! SQL-backed policy reader (plans/m6-privacy.md §6, §70). Versioned legal
//! pages: `current` (latest effective, not superseded) + `history` (all).

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

struct PolicyRow {
    id: i64,
    kind: String,
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
            version: self.version,
            effective_at: self.effective_at,
            superseded_at: self.superseded_at,
            content: self.content,
        })
    }
}

#[async_trait]
impl PolicyReader for SqlxPolicyReader {
    async fn current(&self, kind: PolicyKind) -> Result<Option<PolicyDocument>, PrivacyError> {
        let row = sqlx::query_as!(
            PolicyRow,
            r#"
            SELECT id, kind, version, effective_at, superseded_at, content
            FROM policy_version
            WHERE kind = $1 AND superseded_at IS NULL
            ORDER BY effective_at DESC
            LIMIT 1
            "#,
            kind.as_code(),
        )
        .fetch_optional(self.db.pool())
        .await
        .map_err(map_err)?;
        match row {
            Some(r) => Ok(Some(r.into_document()?)),
            None => Ok(None),
        }
    }

    async fn history(&self, kind: PolicyKind) -> Result<Vec<PolicyDocument>, PrivacyError> {
        let rows = sqlx::query_as!(
            PolicyRow,
            r#"
            SELECT id, kind, version, effective_at, superseded_at, content
            FROM policy_version
            WHERE kind = $1
            ORDER BY effective_at DESC, id DESC
            "#,
            kind.as_code(),
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;
        rows.into_iter().map(PolicyRow::into_document).collect()
    }
}

fn map_err(_e: sqlx::Error) -> PrivacyError {
    PrivacyError::Internal
}
