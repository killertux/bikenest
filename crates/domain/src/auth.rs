//! Accounts & authentication domain value objects (§16–§20).
//!
//! Pure, no I/O. The repository/use-case layers persist and orchestrate these;
//! this module only models account state, roles, providers, passwords and the
//! opaque token/session identifiers.

use crate::{DomainError, UserEmail};
use std::fmt;

/// Account lifecycle state (§20). The suspend/delete *transitions* are M5/M6;
/// M2 defines the enum, implements `PendingEmailVerification → Active`, and
/// *enforces* `Suspended`/`Deleted` at login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountState {
    PendingEmailVerification,
    Active,
    Suspended,
    Deleted,
}

impl AccountState {
    /// Canonical code used in the `users.account_state` column.
    pub fn as_code(self) -> &'static str {
        match self {
            AccountState::PendingEmailVerification => "PENDING_EMAIL_VERIFICATION",
            AccountState::Active => "ACTIVE",
            AccountState::Suspended => "SUSPENDED",
            AccountState::Deleted => "DELETED",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "PENDING_EMAIL_VERIFICATION" => Some(AccountState::PendingEmailVerification),
            "ACTIVE" => Some(AccountState::Active),
            "SUSPENDED" => Some(AccountState::Suspended),
            "DELETED" => Some(AccountState::Deleted),
            _ => None,
        }
    }

    /// May this account authenticate? `PendingEmailVerification` and `Active`
    /// may log in; `Suspended`/`Deleted` are blocked at login (§20).
    pub fn can_log_in(self) -> bool {
        matches!(
            self,
            AccountState::PendingEmailVerification | AccountState::Active
        )
    }

    /// May this account use the authenticated settings pages (C1–C3)?
    pub fn can_access_account(self) -> bool {
        matches!(self, AccountState::PendingEmailVerification | AccountState::Active)
    }

    /// May this account contribute (add/review/verify) — the M2 contract is the
    /// *verified-email* gate; the contribution actions themselves land in M3+.
    pub fn is_verified_gate(&self) -> bool {
        matches!(self, AccountState::Active)
    }
}

/// A granted role (§19). `USER` is the implicit baseline grant at registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    User,
    Moderator,
    Admin,
}

impl Role {
    pub fn as_code(self) -> &'static str {
        match self {
            Role::User => "USER",
            Role::Moderator => "MODERATOR",
            Role::Admin => "ADMIN",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "USER" => Some(Role::User),
            "MODERATOR" => Some(Role::Moderator),
            "ADMIN" => Some(Role::Admin),
            _ => None,
        }
    }
}

/// The kind of login method behind one `authentication_identities` row (§17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthenticationProvider {
    Password,
    Google,
}

impl AuthenticationProvider {
    pub fn as_code(self) -> &'static str {
        match self {
            AuthenticationProvider::Password => "password",
            AuthenticationProvider::Google => "google",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "password" => Some(AuthenticationProvider::Password),
            "google" => Some(AuthenticationProvider::Google),
            _ => None,
        }
    }
}

/// A password credential. Never printed or logged.
#[derive(Clone, PartialEq, Eq)]
pub struct Password(String);

impl Password {
    pub fn new(raw: &str) -> Self {
        Self(raw.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password(•••)")
    }
}

impl fmt::Display for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("•••")
    }
}

/// Password policy (§16): minimum length, no forced character classes.
#[derive(Debug, Clone, Copy)]
pub struct PasswordPolicy {
    pub min_len: usize,
}

impl Default for PasswordPolicy {
    fn default() -> Self {
        Self { min_len: 8 }
    }
}

impl PasswordPolicy {
    /// Validates a candidate password against the policy.
    pub fn validate(&self, pw: &str) -> Result<(), DomainError> {
        if pw.chars().count() < self.min_len {
            return Err(DomainError::WeakPassword);
        }
        Ok(())
    }
}

/// A raw (unhashed) server-side session identifier. Only its SHA-256 hash is
/// persisted; the raw bytes go into the session cookie (§18).
#[derive(Clone, PartialEq, Eq)]
pub struct SessionId([u8; 32]);

impl SessionId {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hex encoding for storing on a cookie.
    pub fn to_hex(&self) -> String {
        bytes_to_hex(&self.0)
    }

    /// Parse a lowercase hex session id from a cookie value.
    pub fn from_hex(s: &str) -> Option<Self> {
        if s.len() != 64 {
            return None;
        }
        let bytes: Option<Vec<u8>> = (0..32)
            .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok())
            .collect();
        let bytes = bytes?;
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Some(Self(arr))
    }
}

impl fmt::Debug for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SessionId({:02x}…)", self.0[0])
    }
}

/// The per-session synchronizer token (CSRF, §18). Stored server-side in the
/// session row; never in a cookie.
#[derive(Clone, PartialEq, Eq)]
pub struct CsrfToken([u8; 32]);

impl CsrfToken {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// URL-safe base64url encoding for a hidden form field / meta tag.
    pub fn to_base64url(&self) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(self.0)
    }

    pub fn from_base64url(s: &str) -> Option<Self> {
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s).ok()?;
        if bytes.len() != 32 {
            return None;
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Some(Self(arr))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Constant-time comparison against a submitted URL-safe base64 value.
    pub fn verify(&self, encoded: &str) -> bool {
        use base64::Engine as _;
        let Ok(decoded) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(encoded) else {
            return false;
        };
        if decoded.len() != self.0.len() {
            return false;
        }
        let mut diff = 0u8;
        for (a, b) in self.0.iter().zip(decoded.iter()) {
            diff |= a ^ b;
        }
        diff == 0
    }
}

impl fmt::Debug for CsrfToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CsrfToken({:02x}…)", self.0[0])
    }
}

/// A single-use email-verification / password-reset token. Only its SHA-256
/// hash is persisted; the raw bytes go into the emailed link (§16).
#[derive(Clone, PartialEq, Eq)]
pub struct VerificationToken([u8; 32]);

impl VerificationToken {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// URL-safe base64url encoding for an emailed link.
    pub fn to_base64url(&self) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(self.0)
    }

    /// Lowercase hex encoding (for tests/keys that key by the raw token).
    pub fn to_hex(&self) -> String {
        bytes_to_hex(&self.0)
    }

    pub fn from_base64url(s: &str) -> Option<Self> {
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s).ok()?;
        if bytes.len() != 32 {
            return None;
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Some(Self(arr))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for VerificationToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VerificationToken({:02x}…)", self.0[0])
    }
}

pub(crate) fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// A Google/other OAuth identity returned by the provider port (§17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderIdentity {
    pub provider: AuthenticationProvider,
    pub subject: String,
    pub email: UserEmail,
    pub email_verified: bool,
}

/// The `User` aggregate now carries the full account lifecycle (§20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: crate::UserId,
    pub email: UserEmail,
    pub display_name: Option<String>,
    pub account_state: AccountState,
    pub email_verified_at: Option<chrono::DateTime<chrono::Utc>>,
    pub roles: Vec<Role>,
}

impl User {
    /// Minimal constructor; a fresh account is `PendingEmailVerification` with
    /// the baseline `USER` role. Callers may adjust fields afterwards.
    pub fn new(id: crate::UserId, email: UserEmail, display_name: Option<String>) -> Self {
        Self {
            id,
            email,
            display_name,
            account_state: AccountState::PendingEmailVerification,
            email_verified_at: None,
            roles: vec![Role::User],
        }
    }

    pub fn has_role(&self, role: Role) -> bool {
        self.roles.contains(&role)
    }

    /// The "may contribute" gate exposed to the session principal (§16).
    pub fn is_verified(&self) -> bool {
        self.email_verified_at.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_state_codes_round_trip() {
        for state in [
            AccountState::PendingEmailVerification,
            AccountState::Active,
            AccountState::Suspended,
            AccountState::Deleted,
        ] {
            assert_eq!(AccountState::from_code(state.as_code()), Some(state));
        }
        assert_eq!(AccountState::from_code("NOPE"), None);
    }

    #[test]
    fn account_state_gates() {
        assert!(AccountState::Active.can_log_in());
        assert!(AccountState::PendingEmailVerification.can_log_in());
        assert!(!AccountState::Suspended.can_log_in());
        assert!(!AccountState::Deleted.can_log_in());
        assert!(!AccountState::Suspended.can_access_account());
        assert!(AccountState::Active.is_verified_gate());
        assert!(!AccountState::PendingEmailVerification.is_verified_gate());
    }

    #[test]
    fn role_codes_round_trip() {
        for role in [Role::User, Role::Moderator, Role::Admin] {
            assert_eq!(Role::from_code(role.as_code()), Some(role));
        }
        assert_eq!(Role::from_code("SUPERUSER"), None);
    }

    #[test]
    fn provider_codes_round_trip() {
        for provider in [AuthenticationProvider::Password, AuthenticationProvider::Google] {
            assert_eq!(
                AuthenticationProvider::from_code(provider.as_code()),
                Some(provider)
            );
        }
        assert_eq!(AuthenticationProvider::from_code("github"), None);
    }

    #[test]
    fn password_policy_boundaries() {
        let policy = PasswordPolicy { min_len: 8 };
        assert_eq!(policy.validate("short"), Err(DomainError::WeakPassword));
        assert_eq!(policy.validate("12345678"), Ok(()));
        assert_eq!(policy.validate("abcdefghij"), Ok(()));
    }

    #[test]
    fn password_is_redacted_from_debug() {
        let pw = Password::new("hunter2secret");
        assert!(format!("{pw:?}").contains("•••"));
        assert!(!format!("{pw:?}").contains("hunter2"));
        assert_eq!(pw.as_str(), "hunter2secret");
    }

    #[test]
    fn token_types_are_distinct() {
        // SessionId and VerificationToken have the same raw shape but are
        // distinct types — a compile-time type-separation guard (no accidental
        // cross-use of a session id where a verification token is expected).
        let raw = [0u8; 32];
        let session = SessionId::new(raw);
        let token = VerificationToken::new(raw);
        assert_eq!(&session.as_bytes()[..], &raw[..]);
        assert_eq!(&token.as_bytes()[..], &raw[..]);
        assert_eq!(session.to_hex(), "00".repeat(32));
        use base64::Engine as _;
        assert_eq!(token.to_base64url(), base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw));
    }

    #[test]
    fn csrf_token_encodes_base64url() {
        let token = CsrfToken::new([1u8; 32]);
        assert!(!token.to_base64url().contains('='));
    }

    #[test]
    fn user_defaults_and_role_checks() {
        let email = crate::UserEmail::parse("a@example.com").unwrap();
        let user = crate::User::new(crate::UserId(1), email, Some("Ada".to_string()));
        assert_eq!(user.account_state, AccountState::PendingEmailVerification);
        assert!(user.has_role(Role::User));
        assert!(!user.has_role(Role::Admin));
        assert!(!user.is_verified());
    }
}
