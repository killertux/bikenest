//! Pure SMTP email provider (`EMAIL_PROVIDER=smtp`) built on `lettre`.
//!
//! Dev/compose points this at **Mailpit** (no TLS, no credentials); a real
//! relay uses `SMTP_TLS=true` (STARTTLS) plus credentials. Swapping backends is
//! a config change, not a code change (§84).

use crate::config::{ConfigError, EmailConfig};

use async_trait::async_trait;
use bikenest_application::{EmailError, EmailProvider, OutboundEmail};
use lettre::message::{Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

type Mailer = AsyncSmtpTransport<Tokio1Executor>;

#[derive(Clone)]
pub struct SmtpEmailProvider {
    mailer: Mailer,
    from: String,
}

impl SmtpEmailProvider {
    pub fn new(
        host: String,
        port: u16,
        username: String,
        password: String,
        from: String,
        tls: bool,
    ) -> Result<Self, EmailError> {
        let mailer = build_mailer(host, port, username, password, tls)?;
        Ok(Self { mailer, from })
    }

    /// Build from the parsed `SMTP_*` block.
    pub fn from_config(config: &EmailConfig) -> Result<Self, ConfigError> {
        let EmailConfig::Smtp {
            host,
            port,
            username,
            password,
            tls,
            from,
        } = config
        else {
            return Err(ConfigError::invalid(
                "EMAIL_PROVIDER",
                "expected the smtp configuration",
            ));
        };
        Self::new(
            host.clone(),
            *port,
            username.clone(),
            password.clone(),
            from.clone(),
            *tls,
        )
        .map_err(|e| ConfigError::invalid("SMTP_HOST", e.to_string()))
    }

    fn message(&self, email: OutboundEmail) -> Result<Message, EmailError> {
        let from: Mailbox = self
            .from
            .parse()
            .map_err(|_| EmailError::Unexpected("invalid EMAIL_FROM".into()))?;
        let to: Mailbox = email
            .to
            .to_string()
            .parse()
            .map_err(|_| EmailError::Unexpected("invalid recipient email".into()))?;
        match email.html {
            Some(html) => Message::builder()
                .from(from)
                .to(to)
                .subject(email.subject)
                .multipart(
                    MultiPart::alternative()
                        .singlepart(SinglePart::plain(email.text))
                        .singlepart(SinglePart::html(html)),
                )
                .map_err(|e| EmailError::Unexpected(e.to_string())),
            None => Message::builder()
                .from(from)
                .to(to)
                .subject(email.subject)
                .body(email.text)
                .map_err(|e| EmailError::Unexpected(e.to_string())),
        }
    }
}

fn build_mailer(
    host: String,
    port: u16,
    username: String,
    password: String,
    tls: bool,
) -> Result<Mailer, EmailError> {
    let builder = if tls {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&host)
            .map_err(|e| EmailError::Unexpected(e.to_string()))?
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&host)
    };
    let builder = builder.port(port);
    let builder = if username.is_empty() {
        builder
    } else {
        builder.credentials(Credentials::new(username, password))
    };
    Ok(builder.build())
}

#[async_trait]
impl EmailProvider for SmtpEmailProvider {
    async fn send(&self, email: OutboundEmail) -> Result<(), EmailError> {
        let message = self.message(email)?;
        self.mailer
            .send(message)
            .await
            .map_err(|e| EmailError::Unexpected(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bikenest_domain::UserEmail;

    fn provider() -> SmtpEmailProvider {
        SmtpEmailProvider::new(
            "smtp.example.com".into(),
            1025,
            String::new(),
            String::new(),
            "no-reply@bikenest.local".into(),
            false,
        )
        .unwrap()
    }

    #[test]
    fn builds_plain_text_message() {
        let email = OutboundEmail {
            to: UserEmail::parse("x@example.com").unwrap(),
            subject: "Hi".into(),
            text: "Body\nPlain".into(),
            html: None,
        };
        let msg = provider().message(email).unwrap();
        assert!(!msg.formatted().is_empty());
    }

    #[test]
    fn builds_alternative_multipart_when_html_present() {
        let email = OutboundEmail {
            to: UserEmail::parse("x@example.com").unwrap(),
            subject: "Hi".into(),
            text: "Body".into(),
            html: Some("<p>Body</p>".into()),
        };
        let msg = provider().message(email).unwrap();
        let bytes = msg.formatted();
        let formatted = String::from_utf8_lossy(&bytes);
        assert!(
            formatted.contains("multipart/alternative"),
            "multipart html email"
        );
    }
}
