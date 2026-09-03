//! Accounts & authentication: ports, read models and the use-case service
//! (§16–§20, §45, §47). Infrastructure implements the ports; the web layer
//! calls [`AuthService`] for every auth/account/role action.

use crate::audit::{AuditEvent, AuditLog};
use crate::email::{EmailProvider, OutboundEmail};
use crate::rate_limit::{RateLimitError, RateLimiter};
use async_trait::async_trait;
use bikenest_domain::{
    AccountState, AuthenticationProvider, CsrfToken, Password, PasswordPolicy, ProviderIdentity,
    Role, SessionId, User, UserEmail, UserId, VerificationToken,
};
use chrono::{DateTime, Utc};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("email already registered")]
    EmailTaken,
    #[error("password does not meet the policy")]
    WeakPassword,
    #[error("invalid email")]
    InvalidEmail,
    #[error("verification token has expired")]
    TokenExpired,
    #[error("verification token has already been used")]
    TokenUsed,
    #[error("invalid verification token")]
    TokenInvalid,
    #[error("too many attempts, try again later")]
    RateLimited,
    #[error("account suspended")]
    AccountSuspended,
    #[error("account deleted")]
    AccountDeleted,
    #[error("identity provider failed")]
    ProviderFailed,
    #[error("you are not permitted to perform this action")]
    Unauthorized,
    #[error("you cannot remove your own last admin role")]
    RefuseAdminSelfRevoke,
    #[error("internal error")]
    Internal,
}

impl From<RateLimitError> for AuthError {
    fn from(_: RateLimitError) -> Self {
        AuthError::RateLimited
    }
}

impl From<crate::email::EmailError> for AuthError {
    fn from(_: crate::email::EmailError) -> Self {
        AuthError::Internal
    }
}

impl From<crate::audit::AuditError> for AuthError {
    fn from(_: crate::audit::AuditError) -> Self {
        AuthError::Internal
    }
}

impl From<bikenest_domain::DomainError> for AuthError {
    fn from(e: bikenest_domain::DomainError) -> Self {
        match e {
            bikenest_domain::DomainError::WeakPassword => AuthError::WeakPassword,
            bikenest_domain::DomainError::EmptyEmail
            | bikenest_domain::DomainError::InvalidEmail(_) => AuthError::InvalidEmail,
            bikenest_domain::DomainError::InvalidRole(_)
            | bikenest_domain::DomainError::InvalidState(_)
            | bikenest_domain::DomainError::Invalid(_) => AuthError::Internal,
        }
    }
}

// ---------------------------------------------------------------------------
// Ports: password hashing, token generation, clock
// ---------------------------------------------------------------------------

/// Port: hash / verify a password (argon2id in M2).
#[async_trait]
pub trait PasswordHasher: Send + Sync {
    async fn hash(&self, pw: &Password) -> Result<String, AuthError>;
    async fn verify(&self, pw: &Password, hash: &str) -> Result<bool, AuthError>;
}

/// Port: cryptographically secure random bytes for tokens/sessions (§16/§18).
pub trait TokenGenerator: Send + Sync {
    fn generate(&self) -> [u8; 32];
}

/// Port: the current time. All expiry logic goes through this (never an inline
/// `Utc::now()`) so tests stay deterministic.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

// ---------------------------------------------------------------------------
// Ports: persistence
// ---------------------------------------------------------------------------

/// One login method row (`authentication_identities`).
#[derive(Debug, Clone)]
pub struct IdentityRecord {
    pub id: i64,
    pub user_id: UserId,
    pub provider: AuthenticationProvider,
    pub provider_subject: String,
    pub credential_hash: Option<String>,
}

/// A new account to create (user + password identity + baseline USER role).
#[derive(Debug)]
pub struct NewAccount<'a> {
    pub email: &'a UserEmail,
    pub display_name: Option<&'a str>,
    pub password_hash: &'a str,
    pub state: AccountState,
}

/// Port: account + role persistence. `Account` is the domain `User` with its
/// roles already loaded.
#[async_trait]
pub trait AccountRepository: Send + Sync {
    async fn find_by_email(&self, email: &UserEmail) -> Result<Option<User>, AuthError>;
    async fn find_by_id(&self, id: UserId) -> Result<Option<User>, AuthError>;
    async fn create(&self, new: NewAccount<'_>) -> Result<UserId, AuthError>;
    async fn set_state(&self, id: UserId, state: AccountState) -> Result<(), AuthError>;
    async fn mark_email_verified(&self, id: UserId, at: DateTime<Utc>) -> Result<(), AuthError>;
    async fn update_canonical_email(&self, id: UserId, email: &UserEmail) -> Result<(), AuthError>;
    /// Atomic confirm: set `email_verified_at`, advance to `Active`, and (when
    /// the address differs) switch `users.email` + the password identity subject
    /// in a single transaction (§2, §20).
    async fn confirm_email(
        &self,
        id: UserId,
        at: DateTime<Utc>,
        email: &UserEmail,
    ) -> Result<(), AuthError>;
    async fn set_password(&self, id: UserId, hash: &str) -> Result<(), AuthError>;
    async fn link_identity(
        &self,
        user_id: UserId,
        provider: AuthenticationProvider,
        subject: &str,
        hash: Option<&str>,
    ) -> Result<(), AuthError>;
    async fn find_identity(
        &self,
        provider: AuthenticationProvider,
        subject: &str,
    ) -> Result<Option<IdentityRecord>, AuthError>;
    async fn roles(&self, id: UserId) -> Result<Vec<Role>, AuthError>;
    async fn grant_role(&self, id: UserId, role: Role, by: UserId) -> Result<(), AuthError>;
    async fn revoke_role(&self, id: UserId, role: Role) -> Result<bool, AuthError>;
    /// All accounts (for the admin user list).
    async fn list_users(&self) -> Result<Vec<User>, AuthError>;
}

/// A resolved server-side session.
#[derive(Debug, Clone)]
pub struct Session {
    pub user_id: UserId,
    pub csrf_token: CsrfToken,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// Port: server-side session store (§18). The cookie carries the raw id; the
/// store persists its SHA-256 hash. `resolve` applies idle + absolute expiry
/// and refreshes `last_seen_at`.
#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn create(
        &self,
        user_id: UserId,
        raw: &SessionId,
        csrf: &CsrfToken,
        now: DateTime<Utc>,
    ) -> Result<(), AuthError>;
    async fn resolve(
        &self,
        raw: &SessionId,
        now: DateTime<Utc>,
    ) -> Result<Option<Session>, AuthError>;
    async fn revoke(&self, raw: &SessionId) -> Result<(), AuthError>;
    async fn revoke_all_for_user_except(
        &self,
        user_id: UserId,
        keep: &SessionId,
    ) -> Result<(), AuthError>;
    /// Revoke every session for a user (the deletion path's "invalidate
    /// sessions" — no session is kept).
    async fn revoke_all_for_user(&self, user_id: UserId) -> Result<(), AuthError>;
}

/// Port: single-use verification / reset token store (§16).
#[async_trait]
pub trait TokenStore: Send + Sync {
    async fn issue_verification(
        &self,
        user_id: UserId,
        email: &str,
        raw: &VerificationToken,
        now: DateTime<Utc>,
    ) -> Result<(), AuthError>;
    async fn consume_verification(
        &self,
        raw: &VerificationToken,
        now: DateTime<Utc>,
    ) -> Result<Option<(UserId, String)>, AuthError>;
    async fn issue_reset(
        &self,
        user_id: UserId,
        raw: &VerificationToken,
        now: DateTime<Utc>,
    ) -> Result<(), AuthError>;
    async fn consume_reset(
        &self,
        raw: &VerificationToken,
        now: DateTime<Utc>,
    ) -> Result<Option<UserId>, AuthError>;
}

/// Port: OAuth provider (§17). The M2 impl is a dev stub (**Ledger #5**).
#[async_trait]
pub trait OAuthProvider: Send + Sync {
    fn authorize_url(&self, state: &str) -> String;
    async fn exchange(&self, code: &str) -> Result<ProviderIdentity, AuthError>;
}

// ---------------------------------------------------------------------------
// Read models for the web layer
// ---------------------------------------------------------------------------

/// An authenticated principal derived from a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedUser {
    pub id: UserId,
    pub email: UserEmail,
    pub display_name: Option<String>,
    pub account_state: AccountState,
    pub is_verified: bool,
    pub roles: Vec<Role>,
}

impl AuthenticatedUser {
    pub fn from_user(u: &User) -> Self {
        Self {
            id: u.id,
            email: u.email.clone(),
            display_name: u.display_name.clone(),
            account_state: u.account_state,
            is_verified: u.is_verified(),
            roles: u.roles.clone(),
        }
    }

    /// The single authorization check used by handlers (§19).
    pub fn has_role(&self, role: Role) -> bool {
        self.roles.contains(&role)
    }
}

/// A session resolved from a cookie, carrying the user + CSRF token.
pub struct ResolvedSession {
    pub user: AuthenticatedUser,
    pub csrf_token: CsrfToken,
}

/// Outcome of a successful sign-in: the raw session id (for the cookie), its
/// CSRF token (for the page) and the authenticated user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginOutcome {
    pub session: SessionId,
    pub csrf: CsrfToken,
    pub user: AuthenticatedUser,
}

// ---------------------------------------------------------------------------
// Rate-limit defaults (§45)
// ---------------------------------------------------------------------------

/// A dummy argon2id PHC string used to equalize login timing when an identity
/// does not exist (the verify call still runs argon2).
const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

// ---------------------------------------------------------------------------
// AuthService
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub struct AuthService {
    accounts: Box<dyn AccountRepository>,
    sessions: Box<dyn SessionStore>,
    tokens: Box<dyn TokenStore>,
    hasher: Box<dyn PasswordHasher>,
    tokens_gen: Box<dyn TokenGenerator>,
    clock: Box<dyn Clock>,
    email: Box<dyn EmailProvider>,
    oauth: Box<dyn OAuthProvider>,
    rate_limiter: Box<dyn RateLimiter>,
    audit: Box<dyn AuditLog>,
    base_url: String,
    password_policy: PasswordPolicy,
}

#[allow(clippy::too_many_arguments)]
impl AuthService {
    pub fn new(
        accounts: Box<dyn AccountRepository>,
        sessions: Box<dyn SessionStore>,
        tokens: Box<dyn TokenStore>,
        hasher: Box<dyn PasswordHasher>,
        tokens_gen: Box<dyn TokenGenerator>,
        clock: Box<dyn Clock>,
        email: Box<dyn EmailProvider>,
        oauth: Box<dyn OAuthProvider>,
        rate_limiter: Box<dyn RateLimiter>,
        audit: Box<dyn AuditLog>,
        base_url: String,
    ) -> Self {
        Self {
            accounts,
            sessions,
            tokens,
            hasher,
            tokens_gen,
            clock,
            email,
            oauth,
            rate_limiter,
            audit,
            base_url,
            password_policy: PasswordPolicy::default(),
        }
    }

    fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    fn verification_link(&self, token: &VerificationToken) -> String {
        format!(
            "{}/verify-email?token={}",
            self.base_url.trim_end_matches('/'),
            token.to_base64url()
        )
    }

    fn reset_link(&self, token: &VerificationToken) -> String {
        format!(
            "{}/password-reset/new?token={}",
            self.base_url.trim_end_matches('/'),
            token.to_base64url()
        )
    }

    /// Assemble the verification email for a token link.
    fn verification_email(&self, to: &UserEmail, token: &VerificationToken) -> OutboundEmail {
        let link = self.verification_link(token);
        OutboundEmail {
            to: to.clone(),
            subject: "Verify your email".to_string(),
            text: format!(
                "Welcome to BikeNest. Confirm your email address to activate your account:\n\n{link}\n\nIf you did not create an account, you can ignore this email."
            ),
            html: None,
        }
    }

    /// Assemble the password-reset email for a token link.
    fn reset_email(&self, to: &UserEmail, token: &VerificationToken) -> OutboundEmail {
        let link = self.reset_link(token);
        OutboundEmail {
            to: to.clone(),
            subject: "Reset your password".to_string(),
            text: format!(
                "We received a request to reset your password. Choose a new one here:\n\n{link}\n\nIf you did not ask for this, you can safely ignore this email."
            ),
            html: None,
        }
    }

    async fn allowed(
        &self,
        key: &str,
        limit: u32,
        window: std::time::Duration,
    ) -> Result<(), AuthError> {
        if self.rate_limiter.check(key, limit, window).await? {
            Ok(())
        } else {
            Err(AuthError::RateLimited)
        }
    }

    // -----------------------------------------------------------------------
    // Register → verify → resend (§16)
    // -----------------------------------------------------------------------

    /// Register an account. Returning `Ok` whether the email is taken or not
    /// (no-existence-leak §45): the email is only sent for a *fresh* signup,
    /// but the caller renders the same "check your inbox" either way.
    pub async fn register(
        &self,
        ip: &str,
        raw_email: &str,
        display_name: Option<&str>,
        raw_password: &str,
    ) -> Result<(), AuthError> {
        self.allowed(
            &format!("register:ip:{ip}"),
            REGISTER_IP_LIMIT,
            std::time::Duration::from_secs(60 * 60),
        )
        .await?;

        let email = UserEmail::parse(raw_email).map_err(|_| AuthError::InvalidEmail)?;
        self.password_policy.validate(raw_password)?;
        let password = Password::new(raw_password);

        let now = self.now();
        // Identical path whether or not the email is taken (§45). If taken, we
        // send no email but still return success — and still burn the same
        // argon2 time, so a timing oracle cannot distinguish an existing
        // account (mirrors the login DUMMY_HASH).
        if self.accounts.find_by_email(&email).await?.is_some() {
            let _ = self.hasher.verify(&password, DUMMY_HASH).await?;
            return Ok(());
        }

        let hash = self.hasher.hash(&password).await?;
        let user_id = self
            .accounts
            .create(NewAccount {
                email: &email,
                display_name,
                password_hash: &hash,
                state: AccountState::PendingEmailVerification,
            })
            .await?;

        let token = VerificationToken::new(self.tokens_gen.generate());
        self.tokens
            .issue_verification(user_id, email.as_str(), &token, now)
            .await?;
        self.email
            .send(self.verification_email(&email, &token))
            .await?;
        self.audit
            .record(AuditEvent::success(
                Some(user_id),
                "auth.register",
                "user",
                user_id.0.to_string(),
            ))
            .await?;
        Ok(())
    }

    /// Verify an email via a single-use token. Handles both registration
    /// (token email == account email → set verified + Active) and change-email
    /// (token email != account email → switch canonical email + verify).
    pub async fn verify_email(&self, raw_token: &str) -> Result<(), AuthError> {
        let now = self.now();
        let token = decode_token(raw_token).ok_or(AuthError::TokenInvalid)?;
        let Some((user_id, email)) = self.tokens.consume_verification(&token, now).await? else {
            return Err(AuthError::TokenInvalid);
        };
        let Some(user) = self.accounts.find_by_id(user_id).await? else {
            return Err(AuthError::TokenInvalid);
        };

        let is_change_email = user.email.as_str() != email;
        let new_email = UserEmail::parse(&email).map_err(|_| AuthError::InvalidEmail)?;
        // One atomic operation (per the plan): set `email_verified_at`, advance
        // to `Active`, and (when changing) switch `users.email` + the password
        // identity subject in a single transaction — one login-lookup key, never
        // divergent (§2, §20).
        self.accounts
            .confirm_email(user_id, now, &new_email)
            .await?;
        if is_change_email {
            // An email change is a security event: invalidate every session so
            // a stale credential on the old address can't keep a session alive.
            self.sessions
                .revoke_all_for_user_except(user_id, &SessionId::new([0u8; 32]))
                .await?;
        }
        let action = if is_change_email {
            "auth.email_changed"
        } else {
            "auth.email_verified"
        };
        self.audit
            .record(AuditEvent::success(
                Some(user_id),
                action,
                "user",
                user_id.0.to_string(),
            ))
            .await?;
        Ok(())
    }

    /// Resend a verification email. Neutral even when no such account exists.
    pub async fn resend_verification(&self, ip: &str, email: &UserEmail) -> Result<(), AuthError> {
        let Some(user) = self.accounts.find_by_email(email).await? else {
            return Ok(());
        };
        self.allowed(
            &format!("verif:user:{}", user.id.0),
            VERIFY_RESEND_USER_LIMIT,
            std::time::Duration::from_secs(60 * 60),
        )
        .await?;
        self.allowed(
            &format!("verif:ip:{ip}"),
            VERIFY_RESEND_IP_LIMIT,
            std::time::Duration::from_secs(60 * 60),
        )
        .await?;

        let token = VerificationToken::new(self.tokens_gen.generate());
        self.tokens
            .issue_verification(user.id, email.as_str(), &token, self.now())
            .await?;
        self.email
            .send(self.verification_email(email, &token))
            .await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Login / logout (§18)
    // -----------------------------------------------------------------------

    /// Sign in by email + password. Returns the same error for bad credentials,
    /// suspended and deleted accounts (no-existence / account-state leak §45).
    /// Always audits the attempt.
    pub async fn login(
        &self,
        ip: &str,
        raw_email: &str,
        raw_password: &str,
    ) -> Result<LoginOutcome, AuthError> {
        let email = UserEmail::parse(raw_email).map_err(|_| AuthError::InvalidCredentials)?;
        self.allowed(
            &format!("login:{ip}:{}", email.as_str()),
            LOGIN_LIMIT,
            std::time::Duration::from_secs(15 * 60),
        )
        .await?;
        self.allowed(
            &format!("login:ip:{ip}"),
            LOGIN_IP_LIMIT,
            std::time::Duration::from_secs(15 * 60),
        )
        .await?;

        let now = self.now();
        let password = Password::new(raw_password);
        let identity_key = email.as_str();

        let Some(identity) = self
            .accounts
            .find_identity(AuthenticationProvider::Password, identity_key)
            .await?
        else {
            // Not found: still run a dummy verify to equalise timing (§45).
            let _ = self.hasher.verify(&password, DUMMY_HASH).await;
            self.audit
                .record(AuditEvent::failure(
                    None,
                    "auth.login",
                    "user",
                    identity_key,
                ))
                .await?;
            return Err(AuthError::InvalidCredentials);
        };

        let Some(hash) = identity.credential_hash.as_deref() else {
            self.audit
                .record(AuditEvent::failure(
                    None,
                    "auth.login",
                    "user",
                    identity_key,
                ))
                .await?;
            return Err(AuthError::InvalidCredentials);
        };
        let ok = self.hasher.verify(&password, hash).await?;
        if !ok {
            self.audit
                .record(AuditEvent::failure(
                    None,
                    "auth.login",
                    "user",
                    identity_key,
                ))
                .await?;
            return Err(AuthError::InvalidCredentials);
        }

        let Some(user) = self.accounts.find_by_id(identity.user_id).await? else {
            self.audit
                .record(AuditEvent::failure(
                    None,
                    "auth.login",
                    "user",
                    identity_key,
                ))
                .await?;
            return Err(AuthError::InvalidCredentials);
        };
        // Suspended / deleted (and any future non-login state) are blocked *at
        // login*, with the generic message — the caller can never tell why.
        if !user.account_state.can_log_in() {
            self.audit
                .record(AuditEvent::failure(
                    None,
                    "auth.login",
                    "user",
                    identity_key,
                ))
                .await?;
            return Err(AuthError::InvalidCredentials);
        }

        let session = SessionId::new(self.tokens_gen.generate());
        let csrf = CsrfToken::new(self.tokens_gen.generate());
        self.sessions.create(user.id, &session, &csrf, now).await?;
        self.audit
            .record(AuditEvent::success(
                Some(user.id),
                "auth.login",
                "user",
                user.id.0.to_string(),
            ))
            .await?;
        Ok(LoginOutcome {
            session,
            csrf,
            user: AuthenticatedUser::from_user(&user),
        })
    }

    /// Revoke the current session.
    pub async fn logout(&self, session: &SessionId) -> Result<(), AuthError> {
        self.sessions.revoke(session).await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Password reset (§16)
    // -----------------------------------------------------------------------

    /// Request a password reset. Neutral (no email) when no such account exists.
    pub async fn request_password_reset(
        &self,
        ip: &str,
        email: &UserEmail,
    ) -> Result<(), AuthError> {
        self.allowed(
            &format!("reset:ip:{ip}"),
            RESET_IP_LIMIT,
            std::time::Duration::from_secs(60 * 60),
        )
        .await?;
        self.allowed(
            &format!("reset:email:{}", email.as_str()),
            RESET_EMAIL_LIMIT,
            std::time::Duration::from_secs(60 * 60),
        )
        .await?;

        let Some(user) = self.accounts.find_by_email(email).await? else {
            return Ok(());
        };
        let token = VerificationToken::new(self.tokens_gen.generate());
        self.tokens.issue_reset(user.id, &token, self.now()).await?;
        self.email.send(self.reset_email(email, &token)).await?;
        Ok(())
    }

    /// Reset the password via a single-use, expiring token; revokes *all*
    /// sessions (§16/§18).
    pub async fn reset_password(
        &self,
        raw_token: &str,
        raw_password: &str,
    ) -> Result<(), AuthError> {
        let now = self.now();
        let token = decode_token(raw_token).ok_or(AuthError::TokenInvalid)?;
        let Some(user_id) = self.tokens.consume_reset(&token, now).await? else {
            return Err(AuthError::TokenInvalid);
        };
        self.password_policy.validate(raw_password)?;
        let hash = self.hasher.hash(&Password::new(raw_password)).await?;
        self.accounts.set_password(user_id, &hash).await?;
        // Revoke every session; on the reset flow the caller is not signed in,
        // so there is no session to keep.
        self.sessions
            .revoke_all_for_user_except(user_id, &SessionId::new([0u8; 32]))
            .await?;
        self.audit
            .record(AuditEvent::success(
                Some(user_id),
                "auth.password_changed",
                "user",
                user_id.0.to_string(),
            ))
            .await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Authenticated settings (C2 / C3)
    // -----------------------------------------------------------------------

    /// Change the password. Requires the current password; revokes all *other*
    /// sessions (keeps `current`).
    pub async fn change_password(
        &self,
        user_id: UserId,
        current: &str,
        new: &str,
        current_session: &SessionId,
    ) -> Result<(), AuthError> {
        let Some(user) = self.accounts.find_by_id(user_id).await? else {
            return Err(AuthError::InvalidCredentials);
        };
        let identity = self
            .accounts
            .find_identity(AuthenticationProvider::Password, user.email.as_str())
            .await?
            .ok_or(AuthError::InvalidCredentials)?;
        let Some(hash) = identity.credential_hash.as_deref() else {
            return Err(AuthError::InvalidCredentials);
        };
        if !self.hasher.verify(&Password::new(current), hash).await? {
            return Err(AuthError::InvalidCredentials);
        }
        self.password_policy.validate(new)?;
        let new_hash = self.hasher.hash(&Password::new(new)).await?;
        self.accounts.set_password(user_id, &new_hash).await?;
        self.sessions
            .revoke_all_for_user_except(user_id, current_session)
            .await?;
        self.audit
            .record(AuditEvent::success(
                Some(user_id),
                "auth.password_changed",
                "user",
                user_id.0.to_string(),
            ))
            .await?;
        Ok(())
    }

    /// Request an email change. Verifies the current password, then issues a
    /// verification token for the *new* address. The actual switch happens in
    /// [`AuthService::verify_email`] when that token is consumed.
    pub async fn change_email(
        &self,
        user_id: UserId,
        current_password: &str,
        new_email: &UserEmail,
    ) -> Result<(), AuthError> {
        let Some(user) = self.accounts.find_by_id(user_id).await? else {
            return Err(AuthError::InvalidCredentials);
        };
        let identity = self
            .accounts
            .find_identity(AuthenticationProvider::Password, user.email.as_str())
            .await?
            .ok_or(AuthError::InvalidCredentials)?;
        let Some(hash) = identity.credential_hash.as_deref() else {
            return Err(AuthError::InvalidCredentials);
        };
        if !self
            .hasher
            .verify(&Password::new(current_password), hash)
            .await?
        {
            return Err(AuthError::InvalidCredentials);
        }
        let token = VerificationToken::new(self.tokens_gen.generate());
        self.tokens
            .issue_verification(user_id, new_email.as_str(), &token, self.now())
            .await?;
        self.email
            .send(self.verification_email(new_email, &token))
            .await?;
        self.audit
            .record(AuditEvent::success(
                Some(user_id),
                "auth.email_change_requested",
                "user",
                user_id.0.to_string(),
            ))
            .await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // OAuth (§17)
    // -----------------------------------------------------------------------

    pub fn oauth_authorize_url(&self, state: &str) -> String {
        self.oauth.authorize_url(state)
    }

    pub async fn oauth_callback(&self, code: &str) -> Result<LoginOutcome, AuthError> {
        let identity = self.oauth.exchange(code).await?;
        let now = self.now();

        // 1) Match by (provider, subject) → existing identity → log in.
        if let Some(rec) = self
            .accounts
            .find_identity(identity.provider, &identity.subject)
            .await?
        {
            let user = self
                .accounts
                .find_by_id(rec.user_id)
                .await?
                .ok_or(AuthError::ProviderFailed)?;
            return self.sign_in(user, now).await;
        }

        // 2) Match by a *verified* email → link identity to that existing account.
        if identity.email_verified
            && let Some(existing) = self.accounts.find_by_email(&identity.email).await?
        {
            self.accounts
                .link_identity(existing.id, identity.provider, &identity.subject, None)
                .await?;
            return self.sign_in(existing, now).await;
        }

        // 3) No match → create a new account (Active only if the provider
        //    asserts a verified email; else pending), then link the identity.
        let state = if identity.email_verified {
            AccountState::Active
        } else {
            AccountState::PendingEmailVerification
        };
        let user_id = self
            .accounts
            .create(NewAccount {
                email: &identity.email,
                display_name: None,
                password_hash: "",
                state,
            })
            .await?;
        self.accounts
            .link_identity(user_id, identity.provider, &identity.subject, None)
            .await?;
        if identity.email_verified {
            self.accounts.mark_email_verified(user_id, now).await?;
        }
        let user = self
            .accounts
            .find_by_id(user_id)
            .await?
            .ok_or(AuthError::ProviderFailed)?;
        self.audit
            .record(AuditEvent::success(
                Some(user_id),
                "auth.oauth_linked",
                "user",
                user_id.0.to_string(),
            ))
            .await?;
        self.sign_in(user, now).await
    }

    async fn sign_in(&self, user: User, now: DateTime<Utc>) -> Result<LoginOutcome, AuthError> {
        // Uniform gate for every OAuth path (match-by-identity, email-link,
        // fresh account): a non-login state cannot establish a session.
        if !user.account_state.can_log_in() {
            return Err(AuthError::InvalidCredentials);
        }
        let session = SessionId::new(self.tokens_gen.generate());
        let csrf = CsrfToken::new(self.tokens_gen.generate());
        self.sessions.create(user.id, &session, &csrf, now).await?;
        self.audit
            .record(AuditEvent::success(
                Some(user.id),
                "auth.login",
                "user",
                user.id.0.to_string(),
            ))
            .await?;
        Ok(LoginOutcome {
            session,
            csrf,
            user: AuthenticatedUser::from_user(&user),
        })
    }

    // -----------------------------------------------------------------------
    // Role management (§19)
    // -----------------------------------------------------------------------

    /// Grant a role. Requires an ADMIN actor; refuses to revoke the actor's own
    /// last ADMIN.
    pub async fn grant_role(
        &self,
        actor: &AuthenticatedUser,
        target: UserId,
        role: Role,
    ) -> Result<(), AuthError> {
        if !actor.has_role(Role::Admin) {
            return Err(AuthError::Unauthorized);
        }
        if role == Role::User {
            return Err(AuthError::Unauthorized);
        }
        self.accounts.grant_role(target, role, actor.id).await?;
        self.audit
            .record(AuditEvent::success(
                Some(actor.id),
                "role.granted",
                "user",
                target.0.to_string(),
            ))
            .await?;
        Ok(())
    }

    /// Revoke a role. Requires an ADMIN actor; refuses to remove the actor's
    /// own last ADMIN.
    pub async fn revoke_role(
        &self,
        actor: &AuthenticatedUser,
        target: UserId,
        role: Role,
    ) -> Result<(), AuthError> {
        if !actor.has_role(Role::Admin) {
            return Err(AuthError::Unauthorized);
        }
        if role == Role::User {
            return Err(AuthError::Unauthorized);
        }
        if target == actor.id && role == Role::Admin {
            let roles = self.accounts.roles(actor.id).await?;
            let admin_count = roles.iter().filter(|r| **r == Role::Admin).count();
            if admin_count <= 1 {
                return Err(AuthError::RefuseAdminSelfRevoke);
            }
        }
        let removed = self.accounts.revoke_role(target, role).await?;
        if removed {
            self.audit
                .record(AuditEvent::success(
                    Some(actor.id),
                    "role.revoked",
                    "user",
                    target.0.to_string(),
                ))
                .await?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Suspend / restore (§20/§44) — ADMIN-only. Suspension revokes every active
    // session so it takes effect immediately, not just at the next login.
    // -----------------------------------------------------------------------

    /// Suspend an account: set `Suspended`, revoke all sessions, audit.
    pub async fn suspend_user(
        &self,
        actor: &AuthenticatedUser,
        target: UserId,
    ) -> Result<(), AuthError> {
        if !actor.has_role(Role::Admin) {
            return Err(AuthError::Unauthorized);
        }
        self.accounts
            .set_state(target, AccountState::Suspended)
            .await?;
        // Revoke every session (keep none): immediate mid-session suspension.
        self.sessions
            .revoke_all_for_user_except(target, &SessionId::new([0u8; 32]))
            .await?;
        self.audit
            .record(AuditEvent::success(
                Some(actor.id),
                "user.suspended",
                "user",
                target.0.to_string(),
            ))
            .await?;
        Ok(())
    }

    /// Restore a suspended account to `Active`, audit.
    pub async fn restore_user(
        &self,
        actor: &AuthenticatedUser,
        target: UserId,
    ) -> Result<(), AuthError> {
        if !actor.has_role(Role::Admin) {
            return Err(AuthError::Unauthorized);
        }
        self.accounts
            .set_state(target, AccountState::Active)
            .await?;
        self.audit
            .record(AuditEvent::success(
                Some(actor.id),
                "user.restored",
                "user",
                target.0.to_string(),
            ))
            .await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Session resolution for middleware
    // -----------------------------------------------------------------------

    /// Resolve a raw session id (from the cookie) to an authenticated principal
    /// and the CSRF token. `None` for a missing / invalid / expired / revoked session
    /// (deny-by-default §19).
    /// All accounts with roles (admin user list, M5).
    pub async fn list_users(&self) -> Result<Vec<AuthenticatedUser>, AuthError> {
        let users = self.accounts.list_users().await?;
        Ok(users.iter().map(AuthenticatedUser::from_user).collect())
    }

    pub async fn resolve_session(
        &self,
        raw: &SessionId,
    ) -> Result<Option<ResolvedSession>, AuthError> {
        let now = self.now();
        let Some(session) = self.sessions.resolve(raw, now).await? else {
            return Ok(None);
        };
        let Some(user) = self.accounts.find_by_id(session.user_id).await? else {
            return Ok(None);
        };
        if !user.account_state.can_access_account() {
            return Ok(None);
        }
        Ok(Some(ResolvedSession {
            user: AuthenticatedUser::from_user(&user),
            csrf_token: session.csrf_token,
        }))
    }
}

fn decode_token(raw: &str) -> Option<VerificationToken> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Some(VerificationToken::new(arr))
}

const REGISTER_IP_LIMIT: u32 = 3;
const LOGIN_LIMIT: u32 = 5;
const LOGIN_IP_LIMIT: u32 = 10;
const RESET_IP_LIMIT: u32 = 3;
const RESET_EMAIL_LIMIT: u32 = 3;
const VERIFY_RESEND_USER_LIMIT: u32 = 3;
const VERIFY_RESEND_IP_LIMIT: u32 = 5;
