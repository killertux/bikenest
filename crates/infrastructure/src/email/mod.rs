//! Generic email-provider implementations (§84). Selected at wiring time by the
//! `EMAIL_PROVIDER` env var (`fake` | `smtp` | `resend`), so swapping backends
//! is a configuration change rather than a domain/app change.

pub mod fake;
pub mod resend;
pub mod smtp;

pub use fake::{CapturedEmail, FakeEmailProvider};
pub use resend::ResendEmailProvider;
pub use smtp::SmtpEmailProvider;

/// Build the email provider selected by `EMAIL_PROVIDER`. Unknown/missing
/// values (and a misconfigured `smtp`/`resend`) fall back to the in-memory fake
/// so `cargo run` and the test harness always work; dev sets `smtp` (Mailpit).
pub fn from_env() -> Box<dyn bikenest_application::EmailProvider> {
    match std::env::var("EMAIL_PROVIDER")
        .unwrap_or_else(|_| "fake".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "smtp" => match SmtpEmailProvider::from_env() {
            Ok(p) => Box::new(p),
            Err(e) => {
                eprintln!(
                    "EMAIL_PROVIDER=smtp but SMTP config is invalid: {e}; falling back to fake"
                );
                Box::new(FakeEmailProvider::new())
            }
        },
        "resend" => match ResendEmailProvider::from_env() {
            Ok(p) => Box::new(p),
            Err(e) => {
                eprintln!(
                    "EMAIL_PROVIDER=resend but Resend config is invalid: {e}; falling back to fake"
                );
                Box::new(FakeEmailProvider::new())
            }
        },
        _ => Box::new(FakeEmailProvider::new()),
    }
}
