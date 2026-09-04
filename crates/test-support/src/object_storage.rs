//! In-memory [`ObjectStorage`] test double.
//!
//! Used by the web/infrastructure test suite so media flows (upload → gallery)
//! run without a filesystem or a real S3/MinIO, storing bytes in a `HashMap`.
//! `presigned_get` returns a `/media/...`-shaped URL purely as a stable,
//! inspectable string for gallery-link assertions — nothing actually serves
//! that path (the app has no media proxy; the real store's presigned URLs
//! point straight at the bucket).

use async_trait::async_trait;
use bikenest_application::{ObjectStorage, PutObject, StorageError};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The URL path prefix under which signed objects are served (matches the app's
/// `/media` route).
pub const MEDIA_BASE_PATH: &str = "/media";

#[derive(Default)]
pub struct TestObjectStorage {
    store: Mutex<HashMap<String, (Vec<u8>, String)>>,
}

impl TestObjectStorage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pre-seed an object so a page/GET can render it without a prior upload.
    pub fn seed(&self, key: &str, bytes: &[u8], content_type: impl Into<String>) {
        self.store
            .lock()
            .unwrap()
            .insert(key.to_string(), (bytes.to_vec(), content_type.into()));
    }

    pub fn len(&self) -> usize {
        self.store.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn contains(&self, key: &str) -> bool {
        self.store.lock().unwrap().contains_key(key)
    }

    /// Reads a stored object's bytes directly (there is no `/media` route to
    /// fetch them through — this is the test-only substitute for that).
    pub fn get_bytes(&self, key: &str) -> Option<Vec<u8>> {
        self.store.lock().unwrap().get(key).map(|(b, _)| b.clone())
    }
}

#[async_trait]
impl ObjectStorage for TestObjectStorage {
    async fn put(&self, req: PutObject<'_>) -> Result<String, StorageError> {
        self.store.lock().unwrap().insert(
            req.key.clone(),
            (req.bytes.to_vec(), req.content_type.clone()),
        );
        Ok(req.key)
    }

    async fn presigned_get(&self, key: &str, ttl: Duration) -> Result<String, StorageError> {
        let exp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
            + ttl.as_secs().max(1);
        // Signature is a sentinel; `verify_get` always accepts (test-only store).
        Ok(format!("{MEDIA_BASE_PATH}/{key}?exp={exp}&sig=testsig"))
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        self.store.lock().unwrap().remove(key);
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        Ok(self.contains(key))
    }
}
