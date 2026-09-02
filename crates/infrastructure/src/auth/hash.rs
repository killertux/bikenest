//! Shared SHA-256 hashing helpers for token/session at-rest storage (§16/§18).
//! Raw random tokens and session ids are stored only as their SHA-256 hex hash,
//! so a database read of `sessions`/`*_tokens` yields no usable credential.

use sha2::{Digest, Sha256};

/// Lowercase hex of the SHA-256 digest of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    to_hex(&hasher.finalize())
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
