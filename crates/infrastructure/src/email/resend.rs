//! Resend API email provider (`EMAIL_PROVIDER=resend`). POSTs to
//! `https://api.resend.com/emails` with the account auth key.

use crate::config::{ConfigError, EmailConfig};

use async_trait::async_trait;
use bikenest_application::{EmailError, EmailProvider, OutboundEmail};
use reqwest::Client;

#[derive(Clone)]
pub struct ResendEmailProvider {
    client: Client,
    api_key: String,
    from: String,
}

impl ResendEmailProvider {
    pub fn new(api_key: impl Into<String>, from: impl Into<String>) -> Self {
        // 10s timeout: an email send runs inline behind a user action (register,
        // resend, password reset); don't let a slow Resend response hang it.
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("reqwest client");
        Self {
            client,
            api_key: api_key.into(),
            from: from.into(),
        }
    }

    /// Build from the parsed `RESEND_*` block.
    pub fn from_config(config: &EmailConfig) -> Result<Self, ConfigError> {
        let EmailConfig::Resend { api_key, from } = config else {
            return Err(ConfigError::invalid(
                "EMAIL_PROVIDER",
                "expected the resend configuration",
            ));
        };
        Ok(Self::new(api_key.clone(), from.clone()))
    }
}

impl ResendEmailProvider {
    fn payload(&self, email: &OutboundEmail) -> serde_json::Value {
        let mut body = serde_json::json!({
            "from": self.from,
            "to": [email.to.to_string()],
            "subject": email.subject,
            "text": email.text,
        });
        if let Some(html) = &email.html {
            body["html"] = serde_json::Value::String(html.clone());
        }
        body
    }
}

#[async_trait]
impl EmailProvider for ResendEmailProvider {
    async fn send(&self, email: OutboundEmail) -> Result<(), EmailError> {
        let body = self.payload(&email);
        let res = self
            .client
            .post("https://api.resend.com/emails")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| EmailError::Unexpected(e.to_string()))?;

        if res.status().is_success() {
            Ok(())
        } else {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            Err(EmailError::Unexpected(format!(
                "resend API {status}: {text}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bikenest_domain::UserEmail;

    fn provider() -> ResendEmailProvider {
        ResendEmailProvider::new("test-key", "no-reply@bikenest.local")
    }

    #[test]
    fn payload_omits_html_when_absent() {
        let email = OutboundEmail {
            to: UserEmail::parse("a@example.com").unwrap(),
            subject: "Subject".into(),
            text: "Body".into(),
            html: None,
        };
        let p = provider().payload(&email);
        assert_eq!(p["from"], "no-reply@bikenest.local");
        assert_eq!(p["to"][0], "a@example.com");
        assert!(p.get("html").is_none());
    }

    #[test]
    fn payload_includes_html_when_present() {
        let email = OutboundEmail {
            to: UserEmail::parse("a@example.com").unwrap(),
            subject: "Subject".into(),
            text: "Body".into(),
            html: Some("<p>Dear</p>".into()),
        };
        let p = provider().payload(&email);
        assert_eq!(p["html"], "<p>Dear</p>");
    }
}
