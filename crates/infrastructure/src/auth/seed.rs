//! `seed-admin` command (**Ledger #10**). Env-driven, idempotent admin
//! bootstrap: reads `ADMIN_EMAIL` / `ADMIN_PASSWORD`, ensures the account is
//! ACTIVE + verified with a password login and USER + ADMIN roles. Never
//! reachable over HTTP.

use crate::Db;
use crate::auth::{Argon2PasswordHasher, SqlxAccountRepository, SqlxAuditLog, SystemClock};
use bikenest_application::{
    AccountRepository, AuditEvent, AuditLog, AuthError, Clock, PasswordHasher,
};
use bikenest_domain::{AccountState, AuthenticationProvider, Password, Role, UserEmail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedOutcome {
    Created,
    Updated,
}

#[derive(Debug, thiserror::Error)]
pub enum SeedAdminError {
    #[error("ADMIN_EMAIL must be set and be a valid email")]
    MissingEmail,
    #[error("ADMIN_PASSWORD must be set (and meet the 8+ char policy)")]
    MissingPassword,
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error("failed to write audit record")]
    Audit(#[from] bikenest_application::AuditError),
}

/// Ensure the env-configured admin exists. Returns whether it was created or
/// updated.
pub async fn seed_admin(db: &Db) -> Result<SeedOutcome, SeedAdminError> {
    let email_raw = std::env::var("ADMIN_EMAIL").map_err(|_| SeedAdminError::MissingEmail)?;
    let email = UserEmail::parse(&email_raw).map_err(|_| SeedAdminError::MissingEmail)?;
    let password_raw =
        std::env::var("ADMIN_PASSWORD").map_err(|_| SeedAdminError::MissingPassword)?;
    bikenest_domain::PasswordPolicy::default()
        .validate(&password_raw)
        .map_err(|_| SeedAdminError::MissingPassword)?;

    let repo = SqlxAccountRepository::new(db.clone());
    let hasher = Argon2PasswordHasher;
    let clock = SystemClock;
    let audit = SqlxAuditLog::new(db.clone());

    let password = Password::new(&password_raw);
    let hash = hasher.hash(&password).await?;
    let now = clock.now();

    let outcome = if let Some(existing) = repo.find_by_email(&email).await? {
        let id = existing.id;
        repo.set_state(id, AccountState::Active).await?;
        repo.mark_email_verified(id, now).await?;
        // Ensure the password identity exists and has the new hash.
        let identity_present = repo
            .find_identity(AuthenticationProvider::Password, email.as_str())
            .await?
            .is_some();
        if identity_present {
            repo.set_password(id, &hash).await?;
        } else {
            repo.link_identity(
                id,
                AuthenticationProvider::Password,
                email.as_str(),
                Some(hash.as_str()),
            )
            .await?;
        }
        repo.grant_role(id, Role::User, id).await?;
        repo.grant_role(id, Role::Admin, id).await?;
        SeedOutcome::Updated
    } else {
        let id = repo
            .create(bikenest_application::NewAccount {
                email: &email,
                display_name: Some("Administrator"),
                password_hash: &hash,
                state: AccountState::Active,
                // Seeded from the CLI, with no page and no request behind it:
                // the product default, changeable from the language toggle.
                locale: bikenest_domain::LocaleCode::default(),
            })
            .await?;
        repo.mark_email_verified(id, now).await?;
        repo.grant_role(id, Role::Admin, id).await?;
        SeedOutcome::Created
    };

    let user = repo
        .find_by_email(&email)
        .await?
        .ok_or(SeedAdminError::MissingEmail)?;
    audit
        .record(AuditEvent::success(
            Some(user.id),
            "admin.seeded",
            "user",
            user.id.0.to_string(),
        ))
        .await?;

    Ok(outcome)
}
