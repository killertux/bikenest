//! SQL-backed server-side session store (§18). The cookie carries the raw id;
//! the DB stores its SHA-256 hash. `resolve` applies idle (30-day) + absolute
//! (90-day) expiry and refreshes `last_seen_at`.

use crate::auth::hash::sha256_hex;
use crate::Db;
use async_trait::async_trait;
use bikenest_application::{AuthError, Session, SessionStore};
use bikenest_domain::{CsrfToken, SessionId, UserId};
use chrono::{DateTime, Duration, Utc};

const INACTIVE_IDLE: Duration = Duration::days(30);
const ABSOLUTE_CAP: Duration = Duration::days(90);

pub struct SqlxSessionStore {
    db: Db,
}

impl SqlxSessionStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

struct SessionRow {
    token_hash: String,
    user_id: i64,
    csrf_token: String,
    created_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

#[async_trait]
impl SessionStore for SqlxSessionStore {
    async fn create(
        &self,
        user_id: UserId,
        raw: &SessionId,
        csrf: &CsrfToken,
        now: DateTime<Utc>,
    ) -> Result<(), AuthError> {
        let token_hash = sha256_hex(raw.as_bytes());
        let expires_at = now + ABSOLUTE_CAP;
        sqlx::query!(
            r#"
            INSERT INTO sessions (token_hash, user_id, csrf_token, expires_at)
            VALUES ($1, $2, $3, $4)
            "#,
            token_hash,
            user_id.0,
            csrf.to_base64url(),
            expires_at,
        )
        .execute(self.db.pool())
        .await
        .map_err(|_| AuthError::Internal)?;
        Ok(())
    }

    async fn resolve(&self, raw: &SessionId, now: DateTime<Utc>) -> Result<Option<Session>, AuthError> {
        let token_hash = sha256_hex(raw.as_bytes());
        let row = sqlx::query_as!(
            SessionRow,
            r#"
            SELECT token_hash, user_id, csrf_token, created_at, last_seen_at, expires_at, revoked_at
            FROM sessions
            WHERE token_hash = $1
            "#,
            token_hash
        )
        .fetch_optional(self.db.pool())
        .await
        .map_err(|_| AuthError::Internal)?;
        let Some(row) = row else {
            return Ok(None);
        };
        if row.revoked_at.is_some() {
            return Ok(None);
        }
        if now > row.expires_at {
            return Ok(None);
        }
        if now - row.last_seen_at > INACTIVE_IDLE {
            return Ok(None);
        }
        // Sliding idle: refresh `last_seen_at`.
        sqlx::query("UPDATE sessions SET last_seen_at = $2 WHERE token_hash = $1")
            .bind(&row.token_hash)
            .bind(now)
            .execute(self.db.pool())
            .await
            .map_err(|_| AuthError::Internal)?;
        let csrf_token = CsrfToken::from_base64url(&row.csrf_token).ok_or(AuthError::Internal)?;
        Ok(Some(Session {
            user_id: UserId(row.user_id),
            csrf_token,
            created_at: row.created_at,
            last_seen_at: row.last_seen_at,
            expires_at: row.expires_at,
            revoked_at: row.revoked_at,
        }))
    }

    async fn revoke(&self, raw: &SessionId) -> Result<(), AuthError> {
        let token_hash = sha256_hex(raw.as_bytes());
        sqlx::query("UPDATE sessions SET revoked_at = now() WHERE token_hash = $1 AND revoked_at IS NULL")
            .bind(token_hash)
            .execute(self.db.pool())
            .await
            .map_err(|_| AuthError::Internal)?;
        Ok(())
    }

    async fn revoke_all_for_user_except(&self, user_id: UserId, keep: &SessionId) -> Result<(), AuthError> {
        let keep_hash = sha256_hex(keep.as_bytes());
        sqlx::query(
            "UPDATE sessions SET revoked_at = now()
             WHERE user_id = $1 AND token_hash != $2 AND revoked_at IS NULL",
        )
        .bind(user_id.0)
        .bind(keep_hash)
        .execute(self.db.pool())
        .await
        .map_err(|_| AuthError::Internal)?;
        Ok(())
    }

    async fn revoke_all_for_user(&self, user_id: UserId) -> Result<(), AuthError> {
        sqlx::query(
            "UPDATE sessions SET revoked_at = now()
             WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(user_id.0)
        .execute(self.db.pool())
        .await
        .map_err(|_| AuthError::Internal)?;
        Ok(())
    }
}
