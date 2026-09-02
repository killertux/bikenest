//! Object-storage port (Ledger #7): opaque byte storage behind a swappable
//! implementation — local disk in dev, S3-compatible later. The port speaks in
//! opaque keys and time-limited ("presigned") GET URLs so replacing the real
//! provider is a wiring change, not a domain change (§84).

use async_trait::async_trait;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("object storage unavailable")]
    Unavailable,
    #[error("object not found")]
    NotFound,
    #[error("object storage error: {0}")]
    Unexpected(String),
}

/// A byte payload to store under a caller-chosen key.
pub struct PutObject<'a> {
    /// Deterministic, opaque key (e.g. `seed/curitiba/12.jpg`). The caller owns
    /// the naming scheme; the store treats the key as opaque.
    pub key: String,
    pub bytes: &'a [u8],
    pub content_type: String,
}

/// Port: store and serve opaque binary objects (photos, in M1/M4).
#[async_trait]
pub trait ObjectStorage: Send + Sync {
    /// Store `req.bytes` under `req.key`, overwriting any existing object.
    /// Returns the stored key (echoed for parity with providers that rewrite it).
    async fn put(&self, req: PutObject<'_>) -> Result<String, StorageError>;

    /// A time-limited GET URL for a stored object (S3-presign parity). This only
    /// signs a URL; it does not touch the backing store, so it is synchronous.
    fn presigned_get(&self, key: &str, ttl: Duration) -> Result<String, StorageError>;

    /// Remove an object. Missing objects are not an error (idempotent).
    async fn delete(&self, key: &str) -> Result<(), StorageError>;

    /// Fetch an object's bytes + content type for serving *through the app*
    /// (local-disk mode: presigned URLs point at our `/media` route). A store
    /// whose presigned URLs bypass the app (S3/CDN) may return
    /// `StorageError::Unexpected` — the app just won't mount a media route.
    async fn get(&self, key: &str) -> Result<(Vec<u8>, String), StorageError>;

    /// Verify a presigned-GET signature+expiry for the `/media` route
    /// (local-disk mode). Returns false when tampered or expired.
    fn verify_get(&self, key: &str, exp: u64, sig: &str) -> bool;
}
