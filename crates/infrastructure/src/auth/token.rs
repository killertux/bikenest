//! Cryptographically secure token generator (§16/§18): 32 random bytes from
//! the OS CSPRNG. A trivial real impl, so no fake exists for it.

use bikenest_application::TokenGenerator;
use rand::RngCore;

/// Uses `rand::rngs::OsRng` to produce 32 random bytes.
#[derive(Debug, Default, Clone, Copy)]
pub struct RealTokenGenerator;

impl TokenGenerator for RealTokenGenerator {
    fn generate(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        bytes
    }
}
