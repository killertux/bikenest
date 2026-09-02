//! Dev-only Google OAuth stub (**Ledger #5**). No real Google credentials — it
//! serves `/auth/google` + `/auth/google/callback` locally with a deterministic
//! identity. The port signature stays compatible with a real Google client
//! (PKCE / state / nonce) that lands in M7.

use async_trait::async_trait;
use bikenest_application::{AuthError, OAuthProvider};
use bikenest_domain::{AuthenticationProvider, ProviderIdentity, UserEmail};

#[derive(Debug, Clone)]
pub struct FakeOAuthProvider {
    email: String,
    subject: String,
}

impl FakeOAuthProvider {
    /// Deterministic identity with the given verified email + subject.
    pub fn new(email: impl Into<String>, subject: impl Into<String>) -> Self {
        Self {
            email: email.into(),
            subject: subject.into(),
        }
    }

    /// Build from `FAKE_OAUTH_EMAIL` / `FAKE_OAUTH_SUB` with dev defaults.
    pub fn from_env() -> Self {
        let email = std::env::var("FAKE_OAUTH_EMAIL")
            .unwrap_or_else(|_| "dev.user@example.com".to_string());
        let subject = std::env::var("FAKE_OAUTH_SUB")
            .unwrap_or_else(|_| "fake-google-sub-000001".to_string());
        Self::new(email, subject)
    }
}

impl Default for FakeOAuthProvider {
    fn default() -> Self {
        Self::from_env()
    }
}

#[async_trait]
impl OAuthProvider for FakeOAuthProvider {
    fn authorize_url(&self, state: &str) -> String {
        // Stub "consent" URL: the browser hits this, and the stub auto-issues a
        // code redirecting to the callback (see the web handler for `/auth/google`).
        format!("/auth/google/fake-consent?state={state}")
    }

    async fn exchange(&self, _code: &str) -> Result<ProviderIdentity, AuthError> {
        let email = UserEmail::parse(&self.email).map_err(|_| AuthError::ProviderFailed)?;
        Ok(ProviderIdentity {
            provider: AuthenticationProvider::Google,
            subject: self.subject.clone(),
            email,
            email_verified: true,
        })
    }
}
