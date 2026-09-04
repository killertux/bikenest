//! In-memory [`ObjectStorage`] test double.
//!
//! Used by the web/infrastructure test suite so media flows (upload → gallery →
//! `/media` GET) run without a filesystem or a real S3/MinIO. It serves objects
//! through the app's `/media/{key}` route with signed-URL parity (immaterial:
//! [`verify_get`](ObjectStorage::verify_get) always accepts), storing bytes in a
//! `HashMap`.

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

    async fn get(&self, key: &str) -> Result<(Vec<u8>, String), StorageError> {
        self.store
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .ok_or(StorageError::NotFound)
    }

    fn verify_get(&self, _key: &str, _exp: u64, _sig: &str) -> bool {
        true
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        Ok(self.contains(key))
    }
}
