//! Audit-log port (§47). Security, account and role actions are recorded here.
//! The M2 implementation writes rows to `audit_events`; the admin *viewer* is a
//! later milestone (M5/M6). No tokens, passwords, or PII beyond actor/target
//! ids reach the audit trail.

use async_trait::async_trait;
use bikenest_domain::UserId;

/// One audit record. `metadata` carries per-action context (never secrets).
#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub actor_user_id: Option<UserId>, // None = system / unauthenticated
    pub action: String,
    pub target_type: String,
    pub target_id: String,
    pub result: String, // "success" | "failure"
    pub metadata: serde_json::Value,
}

impl AuditEvent {
    pub fn new(
        actor: Option<UserId>,
        action: &str,
        target_type: &str,
        target_id: impl Into<String>,
        result: &str,
        metadata: serde_json::Value,
    ) -> Self {
        Self {
            actor_user_id: actor,
            action: action.to_string(),
            target_type: target_type.to_string(),
            target_id: target_id.into(),
            result: result.to_string(),
            metadata,
        }
    }

    pub fn success(
        actor: Option<UserId>,
        action: &str,
        target_type: &str,
        target_id: impl Into<String>,
    ) -> Self {
        Self::new(actor, action, target_type, target_id, "success", serde_json::json!({}))
    }

    pub fn failure(
        actor: Option<UserId>,
        action: &str,
        target_type: &str,
        target_id: impl Into<String>,
    ) -> Self {
        Self::new(actor, action, target_type, target_id, "failure", serde_json::json!({}))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("audit store unavailable")]
    Unavailable,
    #[error("audit store error: {0}")]
    Unexpected(String),
}

/// Port: append an audit record.
#[async_trait]
pub trait AuditLog: Send + Sync {
    async fn record(&self, event: AuditEvent) -> Result<(), AuditError>;
}
