//! In-memory capturing fake (`EMAIL_PROVIDER=fake`). Records every
//! [`OutboundEmail`] in memory (inspectable in tests via
//! [`FakeEmailProvider::emails`]) and, when given an outbox root, appends a
//! capture to `<root>/outbox/` plus `tracing::info!`s the message so the flow
//! can be followed without a real provider.

use async_trait::async_trait;
use bikenest_application::{EmailError, EmailProvider, OutboundEmail};
use std::path::PathBuf;
use std::sync::Mutex;

/// An email this fake has sent.
#[derive(Debug, Clone)]
pub struct CapturedEmail {
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: Option<String>,
}

#[derive(Clone)]
pub struct FakeEmailProvider {
    root: Option<PathBuf>,
    capture: std::sync::Arc<Mutex<Vec<CapturedEmail>>>,
}

impl FakeEmailProvider {
    /// A fake that only captures in memory. The dev outbox on disk is opt-in
    /// via [`FakeEmailProvider::with_root`], which the wiring passes the
    /// configured media root.
    pub fn new() -> Self {
        Self::with_root(None)
    }

    /// A fake with an explicit outbox root (or `None` for in-memory capture
    /// only — used by tests).
    pub fn with_root(root: Option<PathBuf>) -> Self {
        Self {
            root,
            capture: std::sync::Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Everything sent so far, in order.
    pub fn emails(&self) -> Vec<CapturedEmail> {
        self.capture.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// Extract the `token` query param from the first captured email whose text
    /// contains `path` (e.g. `/verify-email` or `/password-reset/new`).
    pub fn token_for(&self, path: &str) -> Option<String> {
        self.emails().iter().find_map(|e| token_from(&e.text, path))
    }

    async fn record(&self, email: &OutboundEmail) {
        let captured = CapturedEmail {
            to: email.to.to_string(),
            subject: email.subject.clone(),
            text: email.text.clone(),
            html: email.html.clone(),
        };
        if let Ok(mut cap) = self.capture.lock() {
            cap.push(captured.clone());
        }
        if let Some(root) = &self.root {
            static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = root.join("outbox");
            let file = dir.join(format!(
                "{}-{n}.txt",
                chrono::Utc::now().format("%Y%m%d%H%M%S")
            ));
            let body = format!(
                "To: {}\nSubject: {}\n\n{}\n",
                email.to, email.subject, email.text
            );
            if tokio::fs::create_dir_all(&dir).await.is_ok()
                && tokio::fs::write(&file, body).await.is_ok()
            {
                tracing::info!(to = %email.to, subject = %email.subject, "fake email captured (Ledger #4)");
            }
        } else {
            tracing::info!(to = %email.to, subject = %email.subject, "fake email captured (Ledger #4)");
        }
    }
}

impl Default for FakeEmailProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EmailProvider for FakeEmailProvider {
    async fn send(&self, email: OutboundEmail) -> Result<(), EmailError> {
        self.record(&email).await;
        Ok(())
    }
}

/// Pull the `token=` value out of a URL that sits within `text`, stopping at the
/// first non-base64url character (the token is URL-safe base64).
fn token_from(text: &str, path: &str) -> Option<String> {
    let url_start = text.find(path)?;
    let rest = &text[url_start..];
    let token_at = rest.find("token=")? + "token=".len();
    let mut end = token_at;
    for (i, b) in rest[token_at..].bytes().enumerate() {
        let is_token_char =
            matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.');
        if !is_token_char {
            end = token_at + i;
            break;
        }
        end = token_at + i + 1;
    }
    Some(rest[token_at..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bikenest_domain::UserEmail;

    #[tokio::test]
    async fn captures_email_and_recovers_token() {
        let fake = FakeEmailProvider::with_root(None);
        fake.send(OutboundEmail {
            to: UserEmail::parse("a@example.com").unwrap(),
            subject: "Verify your email".into(),
            text: "Confirm here: http://localhost:8080/verify-email?token=abc-123_XYZ".into(),
            html: None,
        })
        .await
        .unwrap();

        assert_eq!(fake.emails().len(), 1);
        assert_eq!(
            fake.token_for("/verify-email").as_deref(),
            Some("abc-123_XYZ")
        );
        assert_eq!(fake.token_for("/password-reset/new"), None);
    }
}
