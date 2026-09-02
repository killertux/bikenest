//! SQL-backed audit-log reader (§47) — the admin audit viewer (M6 screen).
//! Filters by actor / action / target / time range; keyset pagination on `id DESC`.

use crate::Db;
use async_trait::async_trait;
use bikenest_application::{AuditError, AuditEvent, AuditFilter, AuditLogReader, AuditPage, AuditStoredEvent};
use bikenest_domain::UserId;
use chrono::{DateTime, Utc};

pub struct SqlxAuditLogReader {
    db: Db,
}

impl SqlxAuditLogReader {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl AuditLogReader for SqlxAuditLogReader {
    async fn list(&self, filter: AuditFilter) -> Result<AuditPage, AuditError> {
        let limit = filter.limit.clamp(1, 100);
        let mut sql = String::from(
            "SELECT id, actor_user_id, action, target_type, target_id, result, metadata, created_at \
             FROM audit_events WHERE true",
        );

        // Build the WHERE clauses + typed binds in lockstep (each clause has
        // exactly one `$n` placeholder, so we simply number them sequentially).
        let mut bind_values: Vec<Bind> = Vec::new();
        let mut clauses: Vec<String> = Vec::new();
        if let Some(actor) = filter.actor_id {
            bind_values.push(Bind::I64(actor.0));
            clauses.push("actor_user_id = $".to_string());
        }
        if let Some(action) = filter.action.as_deref() {
            bind_values.push(Bind::Text(action.to_string()));
            clauses.push("action = $".to_string());
        }
        if let Some(target_type) = filter.target_type.as_deref() {
            bind_values.push(Bind::Text(target_type.to_string()));
            clauses.push("target_type = $".to_string());
        }
        if let Some(from) = filter.from {
            bind_values.push(Bind::Time(from));
            clauses.push("created_at >= $".to_string());
        }
        if let Some(to) = filter.to {
            bind_values.push(Bind::Time(to));
            clauses.push("created_at <= $".to_string());
        }
        if let Some(cursor) = filter.cursor {
            bind_values.push(Bind::I64(cursor));
            clauses.push("id < $".to_string());
        }

        for (i, clause) in clauses.iter().enumerate() {
            sql.push_str(&format!(" AND {clause}{}", i + 1));
        }
        sql.push_str(&format!(" ORDER BY id DESC LIMIT {limit}"));

        let mut query = sqlx::query(&sql);
        for bind in &bind_values {
            match bind {
                Bind::I64(v) => query = query.bind(*v),
                Bind::Text(v) => query = query.bind(v.clone()),
                Bind::Time(v) => query = query.bind(*v),
            }
        }

        let rows = query.fetch_all(self.db.pool()).await.map_err(map_err)?;
        use sqlx::Row;
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let id: i64 = row.try_get("id").map_err(|_| AuditError::Unexpected("missing id".into()))?;
            let actor: Option<i64> = row.try_get("actor_user_id").map_err(map_err)?;
            let action: String = row.try_get("action").map_err(map_err)?;
            let target_type: String = row.try_get("target_type").map_err(map_err)?;
            let target_id: String = row.try_get("target_id").map_err(map_err)?;
            let result: String = row.try_get("result").map_err(map_err)?;
            let metadata: serde_json::Value = row.try_get("metadata").map_err(map_err)?;
            let created_at: DateTime<Utc> = row.try_get("created_at").map_err(map_err)?;
            items.push(AuditStoredEvent {
                id,
                created_at,
                event: AuditEvent {
                    actor_user_id: actor.map(UserId),
                    action,
                    target_type,
                    target_id,
                    result,
                    metadata,
                },
            });
        }
        let next_cursor = items.last().map(|i| i.id);
        Ok(AuditPage { items, next_cursor })
    }
}

enum Bind {
    I64(i64),
    Text(String),
    Time(DateTime<Utc>),
}

fn map_err(_e: sqlx::Error) -> AuditError {
    AuditError::Unexpected("audit read failed".to_string())
}
