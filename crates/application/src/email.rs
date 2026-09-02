//! Generic email-provider port (§84). One `send` method carrying a fully-built
//! [`OutboundEmail`] so any scenario (auth now, notifications later) reuses the
//! same abstraction. Implementations (in-memory fake, SMTP, Resend API) are
//! selected at wiring time via the `EMAIL_PROVIDER` env var — swapping backends
//! is a configuration change, not a domain/app change.

use async_trait::async_trait;
use bikenest_domain::UserEmail;

#[derive(Debug, thiserror::Error)]
pub enum EmailError {
    #[error("mail provider unavailable")]
    Unavailable,
    #[error("mail provider error: {0}")]
    Unexpected(String),
}

/// A ready-to-send transactional email. Callers assemble the subject/body
/// (localized at call time); the provider treats it as opaque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundEmail {
    pub to: UserEmail,
    pub subject: String,
    pub text: String,
    /// Optional HTML version; providers may render a plain-text fallback.
    pub html: Option<String>,
}

/// Port: send one transactional email.
#[async_trait]
pub trait EmailProvider: Send + Sync {
    async fn send(&self, email: OutboundEmail) -> Result<(), EmailError>;
}
