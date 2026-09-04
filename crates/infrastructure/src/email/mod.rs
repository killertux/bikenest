//! Generic email-provider implementations (§84). Selected at wiring time from
//! the parsed `EMAIL_PROVIDER` setting (`fake` | `smtp` | `resend`), so swapping
//! backends is a configuration change rather than a domain/app change.

pub mod fake;
pub mod resend;
pub mod smtp;

pub use fake::{CapturedEmail, FakeEmailProvider};
pub use resend::ResendEmailProvider;
pub use smtp::SmtpEmailProvider;

use crate::config::{ConfigError, EmailConfig};

/// Build the email provider the parsed configuration selected.
///
/// There is no fallback: an SMTP relay that cannot be reached is a startup
/// error, because a silent downgrade to the in-memory fake makes every
/// verification and password-reset message disappear. The fake is chosen only
/// when the configuration asked for it (development default).
pub fn from_config(
    config: &EmailConfig,
) -> Result<Box<dyn bikenest_application::EmailProvider>, ConfigError> {
    match config {
        EmailConfig::Fake { outbox_root } => {
            Ok(Box::new(FakeEmailProvider::with_root(outbox_root.clone())))
        }
        EmailConfig::Smtp { .. } => Ok(Box::new(SmtpEmailProvider::from_config(config)?)),
        EmailConfig::Resend { .. } => Ok(Box::new(ResendEmailProvider::from_config(config)?)),
    }
}
