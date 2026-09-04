//! Object-storage port: opaque byte storage behind an S3-compatible
//! implementation. The port speaks in opaque keys and time-limited
//! ("presigned") GET URLs, generated directly against the bucket — the app
//! never proxies media bytes itself.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
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

/// One object as reported by [`ObjectStorage::list`]: enough to decide whether
/// it is an aged orphan (the key to check against the database, and when it was
/// last written) without fetching any bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectInfo {
    pub key: String,
    pub last_modified: DateTime<Utc>,
}

/// One page of a [`ObjectStorage::list`] walk. `next` carries the key to resume
/// after (a plain key, not a provider-specific token, so every implementation
/// can honour it); `None` means the listing is complete.
#[derive(Debug, Clone, Default)]
pub struct ObjectPage {
    pub objects: Vec<ObjectInfo>,
    pub next: Option<String>,
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

    /// One page of the keys under `prefix`, in ascending key order, resuming
    /// strictly after `after` when given. Metadata only — no bytes are
    /// fetched. This is what makes the retention media sweep possible: the
    /// store, not a filesystem, is the authority on which objects exist.
    ///
    /// A store that cannot answer must return an error; an empty page with no
    /// `next` means "nothing left", and a caller may treat that as fact.
    async fn list(&self, prefix: &str, after: Option<&str>) -> Result<ObjectPage, StorageError>;
}
