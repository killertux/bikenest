//! SQL-backed retention job repository (plans/m6-privacy.md §6, §75).
//!
//! Every purge is a `DELETE WHERE expires_at|revoked_at|cursor < now()` so a
//! re-run is a no-op (idempotent). The two config-gated steps are no-ops when
//! their TTL is 0 (the caller — [`RetentionJob`] — decides whether to invoke
//! them at all; this repo conservatively returns 0 if asked directly).

use crate::Db;
use crate::privacy::SqlxAnonymizationRepository;
use async_trait::async_trait;
use bikenest_application::{
    AnonymizationRepository, ObjectStorage, PrivacyError, RetentionRepository,
};
use bikenest_domain::{RetentionPolicy, UserId};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

pub struct SqlxRetentionRepository {
    db: Db,
    policy: RetentionPolicy,
    storage: Arc<dyn ObjectStorage>,
    media_root: PathBuf,
}

impl SqlxRetentionRepository {
    pub fn new(
        db: Db,
        policy: RetentionPolicy,
        storage: Arc<dyn ObjectStorage>,
        media_root: PathBuf,
    ) -> Self {
        Self {
            db,
            policy,
            storage,
            media_root,
        }
    }
}

/// Classify + log the sqlx error (SQLSTATE, constraint), then map it onto
/// the feature error. `context` names the operation, e.g. `"retention.sweep"`.
fn db_err(context: &'static str, e: sqlx::Error) -> PrivacyError {
    crate::db_error::classify_and_log(context, e).into()
}

// The object's file mtime (SystemTime) is older than `now - ttl`.
fn modified_older_than(mtime: std::time::SystemTime, now: DateTime<Utc>, ttl: Duration) -> bool {
    let cutoff_secs = (now - ttl).timestamp();
    let mtime_secs = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    mtime_secs < cutoff_secs
}

#[async_trait]
impl RetentionRepository for SqlxRetentionRepository {
    async fn purge_expired_password_reset_tokens(
        &self,
        now: DateTime<Utc>,
    ) -> Result<u64, PrivacyError> {
        let res = sqlx::query("DELETE FROM password_reset_tokens WHERE expires_at < $1")
            .bind(now)
            .execute(self.db.pool())
            .await
            .map_err(|e| db_err("retention.purge_expired_password_reset_tokens", e))?;
        Ok(res.rows_affected())
    }

    async fn purge_expired_email_verification_tokens(
        &self,
        now: DateTime<Utc>,
    ) -> Result<u64, PrivacyError> {
        let res = sqlx::query("DELETE FROM email_verification_tokens WHERE expires_at < $1")
            .bind(now)
            .execute(self.db.pool())
            .await
            .map_err(|e| db_err("retention.purge_expired_email_verification_tokens", e))?;
        Ok(res.rows_affected())
    }

    async fn purge_expired_sessions(&self, now: DateTime<Utc>) -> Result<u64, PrivacyError> {
        let idle_cutoff = now - self.policy.session_idle;
        let res = sqlx::query("DELETE FROM sessions WHERE expires_at < $1 OR last_seen_at < $2")
            .bind(now)
            .bind(idle_cutoff)
            .execute(self.db.pool())
            .await
            .map_err(|e| db_err("retention.purge_expired_sessions", e))?;
        Ok(res.rows_affected())
    }

    async fn purge_expired_parked_here(&self, now: DateTime<Utc>) -> Result<u64, PrivacyError> {
        let res = sqlx::query("DELETE FROM verification WHERE kind = 'parked_here' AND expires_at IS NOT NULL AND expires_at < $1").bind(now)
        .execute(self.db.pool())
        .await
        .map_err(|e| db_err("retention.purge_expired_parked_here", e))?;
        Ok(res.rows_affected())
    }

    async fn purge_expired_exports(&self, now: DateTime<Utc>) -> Result<u64, PrivacyError> {
        let res = sqlx::query("DELETE FROM personal_data_export WHERE expires_at < $1")
            .bind(now)
            .execute(self.db.pool())
            .await
            .map_err(|e| db_err("retention.purge_expired_exports", e))?;
        Ok(res.rows_affected())
    }

    async fn purge_orphan_uploads(&self, now: DateTime<Utc>) -> Result<u64, PrivacyError> {
        let referenced = self.referenced_keys().await?;
        let mut purged = 0u64;
        let mut stack = vec![self.media_root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                let file_type = match entry.file_type().await {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                if file_type.is_dir() {
                    stack.push(path);
                    continue;
                }
                if !file_type.is_file() {
                    continue;
                }
                // The object key is the path relative to the media root.
                let Ok(rel) = path.strip_prefix(&self.media_root) else {
                    continue;
                };
                let key = rel.to_string_lossy().replace('\\', "/");
                if referenced.contains(&key) {
                    continue;
                }
                // Only remove objects older than the orphan TTL.
                let Ok(meta) = tokio::fs::metadata(&path).await else {
                    continue;
                };
                if !modified_older_than(
                    meta.modified().unwrap_or(std::time::UNIX_EPOCH),
                    now,
                    self.policy.upload_orphan_ttl,
                ) {
                    continue;
                }
                if self.storage.delete(&key).await.is_ok() {
                    purged += 1;
                }
            }
        }
        Ok(purged)
    }

    async fn anonymize_inactive_accounts(
        &self,
        cutoff: DateTime<Utc>,
    ) -> Result<u64, PrivacyError> {
        #[derive(sqlx::FromRow)]
        struct IdRow {
            id: i64,
        }
        let candidates: Vec<i64> = sqlx::query_as::<_, IdRow>(r#"
            SELECT u.id FROM users u
            WHERE u.account_state <> 'DELETED'
              AND NOT EXISTS (SELECT 1 FROM user_roles r WHERE r.user_id = u.id AND r.role = 'ADMIN')
              AND COALESCE(
                    (SELECT max(s.last_seen_at) FROM sessions s WHERE s.user_id = u.id),
                    u.created_at
                  ) < $1
            "#).bind(cutoff)
        .fetch_all(self.db.pool())
        .await
        .map_err(|e| db_err("retention.anonymize_inactive_accounts", e))?
        .into_iter()
        .map(|r| r.id)
        .collect();

        let anonymizer = SqlxAnonymizationRepository::new(self.db.clone());
        let mut count = 0u64;
        for id in candidates {
            let _ = anonymizer.anonymize(UserId(id), Utc::now()).await?;
            count += 1;
        }
        Ok(count)
    }

    async fn purge_deleted_accounts(&self, cutoff: DateTime<Utc>) -> Result<u64, PrivacyError> {
        let res =
            sqlx::query("DELETE FROM users WHERE account_state = 'DELETED' AND deleted_at < $1")
                .bind(cutoff)
                .execute(self.db.pool())
                .await
                .map_err(|e| db_err("retention.purge_deleted_accounts", e))?;
        Ok(res.rows_affected())
    }
}

impl SqlxRetentionRepository {
    /// The set of object keys still referenced by any photo row (media sweep).
    /// `thumbnail_key` participates as a separate key.
    async fn referenced_keys(&self) -> Result<HashSet<String>, PrivacyError> {
        let mut keys = HashSet::new();
        for row in sqlx::query_scalar::<_, String>(
            "SELECT storage_key FROM parking_photo UNION SELECT storage_key FROM review_photo",
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(|e| db_err("retention.referenced_keys", e))?
        {
            keys.insert(row);
        }
        for k in sqlx::query_scalar::<_, Option<String>>(
            "SELECT thumbnail_key FROM parking_photo WHERE thumbnail_key IS NOT NULL UNION SELECT thumbnail_key FROM review_photo WHERE thumbnail_key IS NOT NULL",
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(|e| db_err("retention.referenced_keys", e))?.into_iter().flatten()
        {
            keys.insert(k);
        }
        Ok(keys)
    }
}
