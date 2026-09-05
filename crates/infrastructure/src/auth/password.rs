//! argon2id password hasher. OWASP memory-hard interactive parameters,
//! per-hash random salt encoded in the PHC string, constant-time verify.
//!
//! Both operations are deliberately expensive (m=19 MiB, t=2) and entirely
//! CPU-bound, so they run on `tokio::task::spawn_blocking` rather than on a
//! runtime worker thread. Hashing inline would park a worker for tens of
//! milliseconds per call — a burst of logins (each of which hashes twice, once
//! for real and once to equalise timing on an unknown address) would otherwise
//! stall every other request on the runtime, `/healthz` included.

use argon2::password_hash::PasswordHasher as ArgonPasswordHasher;
use argon2::{
    Argon2, PasswordHash,
    password_hash::{PasswordVerifier, SaltString, rand_core::OsRng},
};
use async_trait::async_trait;
use bikesnest_application::{AuthError, PasswordHasher};
use bikesnest_domain::Password;

/// Implements [`PasswordHasher`] with argon2id default params
/// (m=19456 KiB, t=2, p=1 — OWASP interactive login baseline).
#[derive(Debug, Default, Clone, Copy)]
pub struct Argon2PasswordHasher;

#[async_trait]
impl PasswordHasher for Argon2PasswordHasher {
    async fn hash(&self, pw: &Password) -> Result<String, AuthError> {
        // The secret is copied into the blocking task; the borrow cannot cross
        // the spawn boundary.
        let secret = pw.as_str().to_string();
        tokio::task::spawn_blocking(move || {
            let salt = SaltString::generate(&mut OsRng);
            Argon2::default()
                .hash_password(secret.as_bytes(), &salt)
                .map(|h| h.to_string())
                .map_err(|_| AuthError::Internal)
        })
        .await
        // A JoinError means the blocking pool panicked or was shut down —
        // indistinguishable from any other internal failure to the caller.
        .map_err(|_| AuthError::Internal)?
    }

    async fn verify(&self, pw: &Password, hash: &str) -> Result<bool, AuthError> {
        let secret = pw.as_str().to_string();
        let hash = hash.to_string();
        tokio::task::spawn_blocking(move || {
            let Ok(parsed) = PasswordHash::new(&hash) else {
                return false;
            };
            Argon2::default()
                .verify_password(secret.as_bytes(), &parsed)
                .is_ok()
        })
        .await
        .map_err(|_| AuthError::Internal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The offload must not need a second runtime thread: `spawn_blocking`
    /// dispatches to the blocking pool, so hashing completes even on a
    /// single-threaded runtime (a naive `block_in_place` would deadlock here).
    #[tokio::test(flavor = "current_thread")]
    async fn hash_and_verify_complete_on_a_single_threaded_runtime() {
        let hasher = Argon2PasswordHasher;
        let pw = Password::new("correct horse battery staple");
        let hash = hasher.hash(&pw).await.expect("hashing succeeds");
        assert!(hash.starts_with("$argon2id$"), "PHC string: {hash}");
        assert!(hasher.verify(&pw, &hash).await.expect("verify runs"));
        assert!(
            !hasher
                .verify(&Password::new("wrong"), &hash)
                .await
                .expect("verify runs")
        );
        // A malformed stored hash is a `false`, never an error.
        assert!(!hasher.verify(&pw, "not-a-phc-string").await.unwrap());
    }
}
