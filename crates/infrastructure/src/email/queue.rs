//! `EmailQueue` implementations: durable (via the job queue) and inline.
//!
//! [`JobEmailQueue`] is the one production wants. `enqueue` is a single INSERT
//! into `background_job` — the same database the account and token rows were
//! just written to — after which the request is done. Delivery, retries with
//! backoff and dead-lettering are the worker's business, so a slow or broken
//! ESP can neither hold an HTTP request open nor fail a registration that has
//! already created an account.
//!
//! [`InlineEmailQueue`] sends on the spot. It exists for two reasons: tests
//! that want the message without running a worker, and deployments with
//! `JOBS_ENABLED=false` — where nothing would ever claim an `email.send` row,
//! so queuing the mail would be the same as dropping it.

use crate::auth::hash::sha256_hex;
use crate::job::repo::SqlxJobRepository;
use async_trait::async_trait;
use bikesnest_application::{EmailError, EmailMessage, EmailProvider, EmailQueue, JOB_EMAIL_SEND};
use std::sync::Arc;

/// Enqueue-time idempotency key for one message: `email:{kind}:{sha256(link)}`.
///
/// The link embeds the single-use token, so the key identifies exactly "this
/// message about this token". A retried enqueue of the same token — a
/// double-submitted form, a retried request — collapses onto the existing row
/// instead of mailing the user twice; a *fresh* token (a real re-send) has a
/// different link and is therefore a different job.
pub fn idempotency_key(msg: &EmailMessage) -> String {
    format!(
        "email:{}:{}",
        msg.kind.code(),
        sha256_hex(msg.kind.link().as_bytes())
    )
}

/// Durable delivery: one `email.send` job per message.
#[derive(Clone)]
pub struct JobEmailQueue {
    jobs: SqlxJobRepository,
    max_attempts: i32,
}

impl JobEmailQueue {
    pub fn new(jobs: SqlxJobRepository, max_attempts: i32) -> Self {
        Self { jobs, max_attempts }
    }
}

#[async_trait]
impl EmailQueue for JobEmailQueue {
    async fn enqueue(&self, msg: EmailMessage) -> Result<(), EmailError> {
        let payload = serde_json::to_value(&msg)
            .map_err(|e| EmailError::Unexpected(format!("email payload: {e}")))?;
        let key = idempotency_key(&msg);
        let queued = self
            .jobs
            .enqueue(
                JOB_EMAIL_SEND,
                &payload,
                chrono::Utc::now(),
                Some(self.max_attempts),
                Some(&key),
            )
            .await
            .map_err(|e| {
                // The caller turns this into a failed request: better than
                // telling someone to check an inbox nothing will arrive in.
                tracing::error!(
                    kind = msg.kind.code(),
                    recipient_domain = msg.recipient_domain(),
                    error = %e,
                    "could not queue transactional email"
                );
                EmailError::Unavailable
            })?;
        match queued {
            Some(id) => tracing::debug!(job = id, kind = msg.kind.code(), "email queued"),
            // The idempotency key already existed: the same message for the
            // same token is pending or was delivered. Not an error.
            None => tracing::debug!(
                kind = msg.kind.code(),
                "email for this token is already queued; not enqueued twice"
            ),
        }
        Ok(())
    }
}

/// Immediate delivery on the calling task. Used by tests and by deployments
/// that run without the background worker.
#[derive(Clone)]
pub struct InlineEmailQueue {
    provider: Arc<dyn EmailProvider>,
}

impl InlineEmailQueue {
    pub fn new(provider: Arc<dyn EmailProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl EmailQueue for InlineEmailQueue {
    async fn enqueue(&self, msg: EmailMessage) -> Result<(), EmailError> {
        self.provider.send(&msg).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bikesnest_application::EmailKind;
    use bikesnest_domain::LocaleCode;

    fn msg(kind: EmailKind) -> EmailMessage {
        EmailMessage::new("ada@example.com", LocaleCode::PtBr, kind)
    }

    #[test]
    fn the_key_is_per_kind_and_per_token() {
        let verify = msg(EmailKind::VerifyEmail {
            link: "https://x/verify-email?token=aaa".into(),
        });
        let same_again = msg(EmailKind::VerifyEmail {
            link: "https://x/verify-email?token=aaa".into(),
        });
        let other_token = msg(EmailKind::VerifyEmail {
            link: "https://x/verify-email?token=bbb".into(),
        });
        let other_kind = msg(EmailKind::ResetPassword {
            link: "https://x/verify-email?token=aaa".into(),
        });

        // Same message twice → one key → the second enqueue is a no-op.
        assert_eq!(idempotency_key(&verify), idempotency_key(&same_again));
        // A re-send issues a new token, and that must be a new job.
        assert_ne!(idempotency_key(&verify), idempotency_key(&other_token));
        // Two different messages about one token stay independent.
        assert_ne!(idempotency_key(&verify), idempotency_key(&other_kind));

        // Shape: no raw token or address in the key (it is stored in a column
        // that is not treated as secret).
        let key = idempotency_key(&verify);
        assert!(key.starts_with("email:verify:"), "{key}");
        assert!(!key.contains("aaa"), "the token must be hashed, not copied");
        assert_eq!(key.len(), "email:verify:".len() + 64);
    }
}
