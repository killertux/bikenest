//! SQL-backed single-use token store (§16). Tokens are stored as SHA-256
//! hashes; single-use is enforced atomically by the `used_at IS NULL` guard in
//! the `UPDATE … RETURNING` (no read-then-write race).

use crate::auth::hash::sha256_hex;
use crate::Db;
use async_trait::async_trait;
use bikenest_application::{AuthError, TokenStore};
use bikenest_domain::{UserId, VerificationToken};
use chrono::{DateTime, Duration, Utc};

const VERIFICATION_TTL: Duration = Duration::hours(24);
const RESET_TTL: Duration = Duration::hours(1);

pub struct SqlxTokenStore {
    db: Db,
}

impl SqlxTokenStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl TokenStore for SqlxTokenStore {
    async fn issue_verification(
        &self,
        user_id: UserId,
        email: &str,
        raw: &VerificationToken,
        now: DateTime<Utc>,
    ) -> Result<(), AuthError> {
        let token_hash = sha256_hex(raw.as_bytes());
        let expires_at = now + VERIFICATION_TTL;
        sqlx::query(r#"
            INSERT INTO email_verification_tokens (token_hash, user_id, email, expires_at)
            VALUES ($1, $2, $3, $4)
            "#).bind(token_hash).bind(user_id.0).bind(email).bind(expires_at)
        .execute(self.db.pool())
        .await
        .map_err(|_| AuthError::Internal)?;
        Ok(())
    }

    async fn consume_verification(
        &self,
        raw: &VerificationToken,
        now: DateTime<Utc>,
    ) -> Result<Option<(UserId, String)>, AuthError> {
        let token_hash = sha256_hex(raw.as_bytes());
#[derive(sqlx::FromRow)]
        struct Row {
            user_id: i64,
            email: String,
        }
        let row = sqlx::query_as::<_, Row>(r#"
            UPDATE email_verification_tokens
            SET used_at = $2
            WHERE token_hash = $1 AND used_at IS NULL AND expires_at > $2
            RETURNING user_id, email
            "#).bind(token_hash).bind(now)
        .fetch_optional(self.db.pool())
        .await
        .map_err(|_| AuthError::Internal)?;
        Ok(row.map(|r| (UserId(r.user_id), r.email)))
    }

    async fn issue_reset(
        &self,
        user_id: UserId,
        raw: &VerificationToken,
        now: DateTime<Utc>,
    ) -> Result<(), AuthError> {
        let token_hash = sha256_hex(raw.as_bytes());
        let expires_at = now + RESET_TTL;
        sqlx::query(r#"
            INSERT INTO password_reset_tokens (token_hash, user_id, expires_at)
            VALUES ($1, $2, $3)
            "#).bind(token_hash).bind(user_id.0).bind(expires_at)
        .execute(self.db.pool())
        .await
        .map_err(|_| AuthError::Internal)?;
        Ok(())
    }

    async fn consume_reset(
        &self,
        raw: &VerificationToken,
        now: DateTime<Utc>,
    ) -> Result<Option<UserId>, AuthError> {
        let token_hash = sha256_hex(raw.as_bytes());
#[derive(sqlx::FromRow)]
        struct Row {
            user_id: i64,
        }
        let row = sqlx::query_as::<_, Row>(r#"
            UPDATE password_reset_tokens
            SET used_at = $2
            WHERE token_hash = $1 AND used_at IS NULL AND expires_at > $2
            RETURNING user_id
            "#).bind(token_hash).bind(now)
        .fetch_optional(self.db.pool())
        .await
        .map_err(|_| AuthError::Internal)?;
        Ok(row.map(|r| UserId(r.user_id)))
    }
}
