//! SQL-backed audit log (§47). Writes rows to `audit_events`.

use crate::Db;
use async_trait::async_trait;
use bikenest_application::{AuditError, AuditEvent, AuditLog};

pub struct SqlxAuditLog {
    db: Db,
}

impl SqlxAuditLog {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl AuditLog for SqlxAuditLog {
    async fn record(&self, event: AuditEvent) -> Result<(), AuditError> {
        sqlx::query!(
            r#"
            INSERT INTO audit_events
                (actor_user_id, action, target_type, target_id, result, metadata)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            event.actor_user_id.map(|u| u.0),
            event.action,
            event.target_type,
            event.target_id,
            event.result,
            event.metadata,
        )
        .execute(self.db.pool())
        .await
        .map_err(|e| AuditError::Unexpected(e.to_string()))?;
        Ok(())
    }
}
