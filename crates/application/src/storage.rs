//! Object-storage port: opaque byte storage behind an S3-compatible
//! implementation. The port speaks in opaque keys and time-limited
//! ("presigned") GET URLs, generated directly against the bucket — the app
//! never proxies media bytes itself.

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

    /// A time-limited GET URL for a stored object. For an S3-compatible store
    /// this returns a *direct* presigned URL (S3 SigV4 signs it; the browser
    /// hits the bucket). Async because the AWS SDK presigner is async.
    async fn presigned_get(&self, key: &str, ttl: Duration) -> Result<String, StorageError>;

    /// Remove an object. Missing objects are not an error (idempotent).
    async fn delete(&self, key: &str) -> Result<(), StorageError>;

    /// Whether `key` currently exists in the store (a metadata-only check —
    /// no bytes are fetched). Used to verify a write actually landed (e.g. the
    /// mock seeder confirming every photo it pushed is really retrievable).
    async fn exists(&self, key: &str) -> Result<bool, StorageError>;
}
