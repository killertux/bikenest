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
        Self::new(
            actor,
            action,
            target_type,
            target_id,
            "success",
            serde_json::json!({}),
        )
    }

    pub fn failure(
        actor: Option<UserId>,
        action: &str,
        target_type: &str,
        target_id: impl Into<String>,
    ) -> Self {
        Self::new(
            actor,
            action,
            target_type,
            target_id,
            "failure",
            serde_json::json!({}),
        )
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

// ---------------------------------------------------------------------------
// Audit reader (§47) — the admin audit-log viewer (M6 screen).
// ---------------------------------------------------------------------------

/// Filter for the audit viewer. All fields optional; each present field is ANDed.
#[derive(Debug, Clone, Default)]
pub struct AuditFilter {
    pub actor_id: Option<bikenest_domain::UserId>,
    pub action: Option<String>,
    pub target_type: Option<String>,
    pub from: Option<chrono::DateTime<chrono::Utc>>,
    pub to: Option<chrono::DateTime<chrono::Utc>>,
    /// Keyset cursor on `id DESC`: the last seen event id.
    pub cursor: Option<i64>,
    pub limit: usize,
}

/// One page of audit events (keyset pagination on `id DESC`).
#[derive(Debug, Clone)]
pub struct AuditPage {
    pub items: Vec<AuditStoredEvent>,
    /// Present when another page exists.
    pub next_cursor: Option<i64>,
}

/// A single audit row as read back, with its id + created_at.
#[derive(Debug, Clone)]
pub struct AuditStoredEvent {
    pub id: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub event: AuditEvent,
}

/// Port: read the audit trail. The viewer is ADMIN-only; metadata is rendered
/// as an escaped JSON blob and by construction carries no secrets/PII (§47).
#[async_trait]
pub trait AuditLogReader: Send + Sync {
    async fn list(&self, filter: AuditFilter) -> Result<AuditPage, AuditError>;
}
