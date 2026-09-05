//! The `email.send` job handler: one queued message → one provider call.
//!
//! Retries, backoff and dead-lettering come from the queue, so this handler
//! only has to classify: a provider error is transient (`Failed`, retry within
//! the budget), an undecodable payload is not (`Permanent` — no number of
//! retries will make it parse).
//!
//! At-least-once execution is real here, and unlike a purge or an upsert a send
//! cannot be undone. What keeps a user from getting the same mail twice is the
//! enqueue-time idempotency key (`email:{kind}:{sha256(link)}`): the row for a
//! given token exists once, so the message is *queued* once. A lease that
//! expires mid-send can still deliver twice — the alternative (marking sent
//! before sending) drops mail instead, and a duplicate verification link is
//! the better failure.

use async_trait::async_trait;
use bikesnest_application::{
    EmailMessage, EmailProvider, JOB_EMAIL_SEND, JobError, JobHandler, JobPayload,
};
use std::sync::Arc;

pub struct SendEmailHandler {
    provider: Arc<dyn EmailProvider>,
}

impl SendEmailHandler {
    pub fn new(provider: Arc<dyn EmailProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl JobHandler for SendEmailHandler {
    fn kind(&self) -> &'static str {
        JOB_EMAIL_SEND
    }

    async fn run(&self, payload: &JobPayload) -> Result<(), JobError> {
        let msg = decode(payload)?;
        self.provider.send(&msg).await.map_err(|e| {
            JobError::Failed(format!(
                "sending {} to {} failed: {e}",
                msg.kind.code(),
                msg.recipient_domain()
            ))
        })
    }

    /// A dead-lettered email is a user who is stuck: no verification link, no
    /// password reset. Log it at `error!` so it is alertable — with the message
    /// kind and the recipient's *domain* only. The address itself is personal
    /// data and the link is a live credential; neither belongs in a log line.
    async fn on_dead_letter(&self, payload: &JobPayload, error: &str) {
        match decode(payload) {
            Ok(msg) => tracing::error!(
                kind = msg.kind.code(),
                recipient_domain = msg.recipient_domain(),
                locale = msg.locale.as_str(),
                error,
                "transactional email dead-lettered; the recipient never got it"
            ),
            Err(_) => tracing::error!(error, "email.send dead-lettered with an unreadable payload"),
        }
    }
}

/// Decode the queued payload. A shape mismatch means the row was written by
/// another version of the app (or by hand): permanent, not retryable.
fn decode(payload: &JobPayload) -> Result<EmailMessage, JobError> {
    serde_json::from_value(payload.clone())
        .map_err(|e| JobError::Permanent(format!("unreadable email.send payload: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::email::FakeEmailProvider;
    use bikesnest_application::{EmailError, EmailKind};
    use bikesnest_domain::LocaleCode;

    fn payload(locale: LocaleCode) -> JobPayload {
        serde_json::to_value(EmailMessage::new(
            "ada@example.com",
            locale,
            EmailKind::VerifyEmail {
                link: "https://bikesnest.test/verify-email?token=t".into(),
            },
        ))
        .unwrap()
    }

    /// A provider that always fails, to check the error classification.
    struct BrokenProvider;
    #[async_trait]
    impl EmailProvider for BrokenProvider {
        async fn send(&self, _msg: &EmailMessage) -> Result<(), EmailError> {
            Err(EmailError::Unavailable)
        }
    }

    #[tokio::test]
    async fn runs_the_payload_through_the_provider_in_its_locale() {
        let fake = FakeEmailProvider::with_root(None);
        let handler = SendEmailHandler::new(Arc::new(fake.clone()));
        handler.run(&payload(LocaleCode::PtBr)).await.unwrap();

        let sent = fake.emails();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].locale, "pt-BR");
        assert_eq!(sent[0].subject, "Confirme seu e-mail no BikesNest");
    }

    #[tokio::test]
    async fn a_provider_failure_is_transient_so_the_queue_retries() {
        let handler = SendEmailHandler::new(Arc::new(BrokenProvider));
        let err = handler.run(&payload(LocaleCode::En)).await.unwrap_err();
        assert!(matches!(err, JobError::Failed(_)), "{err:?}");
        // The error text names the kind and the domain, never the address.
        let text = err.to_string();
        assert!(
            text.contains("verify") && text.contains("example.com"),
            "{text}"
        );
        assert!(!text.contains("ada@"), "the address must not reach a log");
    }

    #[tokio::test]
    async fn an_unreadable_payload_is_permanent_so_it_is_not_retried() {
        let handler = SendEmailHandler::new(Arc::new(FakeEmailProvider::with_root(None)));
        let err = handler
            .run(&serde_json::json!({"to": "a@example.com"}))
            .await
            .unwrap_err();
        assert!(matches!(err, JobError::Permanent(_)), "{err:?}");
    }
}
