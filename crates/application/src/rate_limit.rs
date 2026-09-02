//! Rate-limiter port (§45). Introduced in M2 for the authentication endpoints
//! (login, register, password reset, verification resend) so authentication
//! never ships without brute-force / account-enumeration protection. The
//! contribution-endpoint limits (M3) reuse the same port. **Ledger #6:** the
//! M2 implementation is in-memory; a Redis/shared store replaces it in M7.

use async_trait::async_trait;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum RateLimitError {
    #[error("rate limiter unavailable")]
    Unavailable,
    #[error("rate limiter error: {0}")]
    Unexpected(String),
}

/// Port: enforce a sliding-window limit on a key.
///
/// `Ok(true)` = allowed; `Ok(false)` = over the limit (`limit` hits within
/// `window`).
#[async_trait]
pub trait RateLimiter: Send + Sync {
    async fn check(&self, key: &str, limit: u32, window: Duration) -> Result<bool, RateLimitError>;
}
