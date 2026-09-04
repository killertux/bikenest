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

    /// Walks the media tree and deletes aged, unreferenced files. Referenced-
    /// ness is probed in batches (`referenced`) rather than loading every
    /// photo key in the database up front — the age gate runs first so only
    /// candidates that would actually be deleted ever reach the DB.
    ///
    /// (This walks a local filesystem; WP16 rewrites the sweep against object
    /// storage directly. This change is deliberately minimal and compatible
    /// with that follow-up — only the referenced-keys check is batched.)
    async fn purge_orphan_uploads(&self, now: DateTime<Utc>) -> Result<u64, PrivacyError> {
        const BATCH_SIZE: usize = 500;
        let mut purged = 0u64;
        let mut stack = vec![self.media_root.clone()];
        let mut batch: Vec<String> = Vec::with_capacity(BATCH_SIZE);
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
                // Only candidates older than the orphan TTL are worth a
                // referenced-ness check at all.
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
                // The object key is the path relative to the media root.
                let Ok(rel) = path.strip_prefix(&self.media_root) else {
                    continue;
                };
                batch.push(rel.to_string_lossy().replace('\\', "/"));
                if batch.len() >= BATCH_SIZE {
                    purged += self.purge_unreferenced(std::mem::take(&mut batch)).await?;
                }
            }
        }
        if !batch.is_empty() {
            purged += self.purge_unreferenced(batch).await?;
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
    /// Of `candidate_keys`, the subset still referenced by some photo row
    /// (as either `storage_key` or `thumbnail_key`) — probed in one query per
    /// batch instead of loading every referenced key in the database. Safe to
    /// call with an empty batch (short-circuits without a round trip).
    async fn referenced(&self, candidate_keys: &[String]) -> Result<HashSet<String>, PrivacyError> {
        if candidate_keys.is_empty() {
            return Ok(HashSet::new());
        }
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT storage_key FROM parking_photo WHERE storage_key = ANY($1)
            UNION
            SELECT storage_key FROM review_photo WHERE storage_key = ANY($1)
            UNION
            SELECT thumbnail_key FROM parking_photo WHERE thumbnail_key = ANY($1)
            UNION
            SELECT thumbnail_key FROM review_photo WHERE thumbnail_key = ANY($1)
            "#,
        )
        .bind(candidate_keys)
        .fetch_all(self.db.pool())
        .await
        .map_err(|e| db_err("retention.referenced", e))?;
        Ok(rows.into_iter().collect())
    }

    /// Deletes every key in `candidates` that [`Self::referenced`] does not
    /// report as still in use. Returns the number actually deleted.
    async fn purge_unreferenced(&self, candidates: Vec<String>) -> Result<u64, PrivacyError> {
        let referenced = self.referenced(&candidates).await?;
        let mut purged = 0u64;
        for key in candidates {
            if referenced.contains(&key) {
                continue;
            }
            if self.storage.delete(&key).await.is_ok() {
                purged += 1;
            }
        }
        Ok(purged)
    }
}
