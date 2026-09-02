//! Local-filesystem object storage (Ledger #7).
//!
//! Implements the [`ObjectStorage`] port by writing bytes under a root
//! directory and issuing HMAC-signed, time-limited GET URLs that point at the
//! app's own `/media/{key}` route (S3-presign parity). Replacing this with an
//! S3-compatible store in M7 is a wiring change: the presigned URLs would then
//! point straight at the bucket and the `/media` route would not be mounted.

use async_trait::async_trait;
use bikenest_application::{ObjectStorage, PutObject, StorageError};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

/// The URL path prefix under which signed objects are served.
pub const MEDIA_BASE_PATH: &str = "/media";

#[derive(Clone)]
pub struct LocalDiskStorage {
    root: PathBuf,
    secret: Vec<u8>,
}

impl LocalDiskStorage {
    pub fn new(root: impl Into<PathBuf>, secret: impl Into<Vec<u8>>) -> Self {
        Self {
            root: root.into(),
            secret: secret.into(),
        }
    }

    /// Build from the environment (`MEDIA_ROOT`, `MEDIA_SIGNING_SECRET`) with
    /// dev defaults so `cargo run` and the test harness work without config.
    pub fn from_env() -> Self {
        let root = std::env::var("MEDIA_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../media"))
            });
        let secret = std::env::var("MEDIA_SIGNING_SECRET")
            .unwrap_or_else(|_| "dev-insecure-media-signing-secret".to_string());
        Self::new(root, secret.into_bytes())
    }

    /// Resolve a key to a path under `root`, rejecting traversal (`..`, absolute
    /// keys, backslashes).
    fn safe_path(&self, key: &str) -> Result<PathBuf, StorageError> {
        if key.is_empty()
            || key.starts_with('/')
            || key.contains("..")
            || key.contains('\\')
        {
            return Err(StorageError::Unexpected(format!("unsafe key: {key}")));
        }
        Ok(self.root.join(key))
    }

    fn mac(&self, key: &str, exp: u64) -> HmacSha256 {
        let mut mac =
            HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts any key length");
        mac.update(key.as_bytes());
        mac.update(b"\n");
        mac.update(exp.to_string().as_bytes());
        mac
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// Best-effort content type from a key's extension (local store keeps no
/// sidecar metadata; the seeded/uploaded keys always carry an extension).
fn content_type_for(key: &str) -> String {
    let ext = Path::new(key)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[async_trait]
impl ObjectStorage for LocalDiskStorage {
    async fn put(&self, req: PutObject<'_>) -> Result<String, StorageError> {
        let path = self.safe_path(&req.key)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }
        tokio::fs::write(&path, req.bytes)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(req.key)
    }

    fn presigned_get(&self, key: &str, ttl: Duration) -> Result<String, StorageError> {
        // Signing does not touch the store, but reject obviously-bad keys early.
        let _ = self.safe_path(key)?;
        let exp = unix_now() + ttl.as_secs().max(1);
        let sig = to_hex(&self.mac(key, exp).finalize().into_bytes());
        Ok(format!("{MEDIA_BASE_PATH}/{key}?exp={exp}&sig={sig}"))
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let path = self.safe_path(key)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StorageError::Unexpected(e.to_string())),
        }
    }

    async fn get(&self, key: &str) -> Result<(Vec<u8>, String), StorageError> {
        let path = self.safe_path(key)?;
        match tokio::fs::read(&path).await {
            Ok(bytes) => Ok((bytes, content_type_for(key))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(StorageError::NotFound),
            Err(e) => Err(StorageError::Unexpected(e.to_string())),
        }
    }

    fn verify_get(&self, key: &str, exp: u64, sig: &str) -> bool {
        if exp < unix_now() {
            return false;
        }
        let Some(provided) = from_hex(sig) else {
            return false;
        };
        // `verify_slice` is constant-time.
        self.mac(key, exp).verify_slice(&provided).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> LocalDiskStorage {
        LocalDiskStorage::new(std::env::temp_dir().join("bikenest-media-test"), b"secret".to_vec())
    }

    #[test]
    fn presigned_url_verifies_and_detects_tampering() {
        let s = store();
        let url = s.presigned_get("seed/x.jpg", Duration::from_secs(60)).unwrap();
        // Parse exp+sig back out of the URL.
        let (_, qs) = url.split_once('?').unwrap();
        let mut exp = 0u64;
        let mut sig = String::new();
        for kv in qs.split('&') {
            let (k, v) = kv.split_once('=').unwrap();
            match k {
                "exp" => exp = v.parse().unwrap(),
                "sig" => sig = v.to_string(),
                _ => {}
            }
        }
        assert!(s.verify_get("seed/x.jpg", exp, &sig));
        assert!(!s.verify_get("seed/other.jpg", exp, &sig), "key tamper rejected");
        assert!(!s.verify_get("seed/x.jpg", exp, "deadbeef"), "sig tamper rejected");
    }

    #[test]
    fn expired_signature_is_rejected() {
        let s = store();
        let sig = to_hex(&s.mac("k.jpg", 1).finalize().into_bytes());
        assert!(!s.verify_get("k.jpg", 1, &sig), "past expiry rejected");
    }

    #[test]
    fn rejects_path_traversal_keys() {
        let s = store();
        assert!(s.safe_path("../etc/passwd").is_err());
        assert!(s.safe_path("/abs").is_err());
        assert!(s.safe_path("ok/nested.jpg").is_ok());
    }

    #[tokio::test]
    async fn put_get_delete_round_trip() {
        let s = store();
        let key = "seed/roundtrip.jpg".to_string();
        s.put(PutObject { key: key.clone(), bytes: b"hello", content_type: "image/jpeg".into() })
            .await
            .unwrap();
        let (bytes, ct) = s.get(&key).await.unwrap();
        assert_eq!(bytes, b"hello");
        assert_eq!(ct, "image/jpeg");
        s.delete(&key).await.unwrap();
        assert!(matches!(s.get(&key).await, Err(StorageError::NotFound)));
        // delete is idempotent
        s.delete(&key).await.unwrap();
    }
}
