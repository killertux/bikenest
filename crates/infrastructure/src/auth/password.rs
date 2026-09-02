//! argon2id password hasher (§16). OWASP memory-hard interactive parameters,
//! per-hash random salt encoded in the PHC string, constant-time verify.

use argon2::{
    Argon2, PasswordHash,
    password_hash::{PasswordVerifier, SaltString, rand_core::OsRng},
};
use argon2::password_hash::PasswordHasher as ArgonPasswordHasher;
use async_trait::async_trait;
use bikenest_application::{AuthError, PasswordHasher};
use bikenest_domain::Password;

/// Implements [`PasswordHasher`] with argon2id default params
/// (m=19456 KiB, t=2, p=1 — OWASP interactive login baseline).
#[derive(Debug, Default, Clone, Copy)]
pub struct Argon2PasswordHasher;

#[async_trait]
impl PasswordHasher for Argon2PasswordHasher {
    async fn hash(&self, pw: &Password) -> Result<String, AuthError> {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(pw.as_str().as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|_| AuthError::Internal)
    }

    async fn verify(&self, pw: &Password, hash: &str) -> Result<bool, AuthError> {
        let Ok(parsed) = PasswordHash::new(hash) else {
            return Ok(false);
        };
        Ok(Argon2::default()
            .verify_password(pw.as_str().as_bytes(), &parsed)
            .is_ok())
    }
}
