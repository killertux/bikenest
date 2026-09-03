//! S3-compatible object storage (**Ledger #7**).
//!
//! Replaces the local-disk store with an S3-compatible backend (AWS S3,
//! Cloudflare R2, Backblaze B2, MinIO). Writes go to the bucket; object reads
//! are served through the app's `/media/{key}` route using HMAC-signed,
//! time-limited GET URLs (the port's "presigned GET parity") — the bucket stays
//! private and the app authorizes every media read. The store takes keys as
//! opaque strings and keeps S3's content-type metadata.
//!
//! Note: we deliberately serve media *through the app* (rather than handing the
//! browser a direct presigned bucket URL). This keeps the `presigned_get`
//! port method synchronous (it is HMAC signing, not an async S3 presign call)
//! and preserves the existing signed-URL access-control model. To offload media
//! to the bucket directly, the port would need `presigned_get` made async.
//!
//! Configured with `S3_*` env (dev defaults target a local MinIO):
//! - `S3_ENDPOINT` — default `http://localhost:9000`; set `S3_ENDPOINT=` (empty)
//!   to use the standard AWS endpoint.
//! - `S3_REGION` — default `us-east-1`
//! - `S3_BUCKET` — default `bikenest`
//! - `S3_ACCESS_KEY_ID` / `S3_SECRET_ACCESS_KEY` — default MinIO `minioadmin`
//! - `MEDIA_SIGNING_SECRET` — required to sign `/media` URLs; dev-insecure default
//!   (must be overridden — Ledger #14).

use async_trait::async_trait;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::Config;
use aws_smithy_types::byte_stream::ByteStream;
use bikenest_application::{ObjectStorage, PutObject, StorageError};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DEFAULT_S3_REGION: &str = "us-east-1";
pub const DEFAULT_S3_BUCKET: &str = "bikenest";
/// The URL path prefix under which signed objects are served.
pub const MEDIA_BASE_PATH: &str = "/media";

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct S3ObjectStorage {
    client: aws_sdk_s3::Client,
    bucket: String,
    /// HMAC key for `/media` signed URLs (must be a strong secret in prod).
    secret: Vec<u8>,
}

impl S3ObjectStorage {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        endpoint: Option<String>,
        region: String,
        bucket: String,
        access_key: String,
        secret_key: String,
        signing_secret: impl Into<Vec<u8>>,
    ) -> Self {
        let mut builder = Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(region))
            .credentials_provider(Credentials::new(
                access_key,
                secret_key,
                None,
                None,
                "bikenest",
            ))
            .force_path_style(true);
        if let Some(endpoint) = endpoint {
            builder = builder.endpoint_url(endpoint);
        }
        Self {
            client: aws_sdk_s3::Client::from_conf(builder.build()),
            bucket,
            secret: signing_secret.into(),
        }
    }

    /// Build from the `S3_*` env vars, defaulting to a local MinIO so `cargo run`
    /// works against the compose stack (dev). Production sets real credentials
    /// and a real `MEDIA_SIGNING_SECRET`.
    pub fn from_env() -> Self {
        let endpoint = std::env::var("S3_ENDPOINT")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| Some("http://localhost:9000".to_string()));
        let region = std::env::var("S3_REGION").unwrap_or_else(|_| DEFAULT_S3_REGION.to_string());
        let bucket = std::env::var("S3_BUCKET").unwrap_or_else(|_| DEFAULT_S3_BUCKET.to_string());
        let access =
            std::env::var("S3_ACCESS_KEY_ID").unwrap_or_else(|_| "minioadmin".to_string());
        let secret =
            std::env::var("S3_SECRET_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_string());
        let signing_secret = std::env::var("MEDIA_SIGNING_SECRET")
            .unwrap_or_else(|_| "dev-insecure-media-signing-secret".to_string());
        Self::new(endpoint, region, bucket, access, secret, signing_secret)
    }

    /// Reject keys that would break the `/media/{key}` URL (`?`/`#` keep the
    /// query/URL from parsing cleanly). Keys are otherwise opaque.
    fn safe_key(&self, key: &str) -> Result<(), StorageError> {
        if key.is_empty() || key.contains(['?', '#', '\n', '\r']) {
            return Err(StorageError::Unexpected(format!("unsafe key: {key}")));
        }
        Ok(())
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

/// Build the S3 storage from the `S3_*` env vars (see [`S3ObjectStorage::from_env`]).
pub fn storage_from_env() -> S3ObjectStorage {
    S3ObjectStorage::from_env()
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

#[async_trait]
impl ObjectStorage for S3ObjectStorage {
    async fn put(&self, req: PutObject<'_>) -> Result<String, StorageError> {
        self.safe_key(&req.key)?;
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&req.key)
            .content_type(&req.content_type)
            .body(ByteStream::from(req.bytes.to_vec()))
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, key = %req.key, "S3 put failed");
                StorageError::Unavailable
            })?;
        Ok(req.key)
    }

    fn presigned_get(&self, key: &str, ttl: Duration) -> Result<String, StorageError> {
        // Signing does not touch the store — pure HMAC + URL assembly.
        self.safe_key(key)?;
        let exp = unix_now() + ttl.as_secs().max(1);
        let sig = to_hex(&self.mac(key, exp).finalize().into_bytes());
        Ok(format!("{MEDIA_BASE_PATH}/{key}?exp={exp}&sig={sig}"))
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        match self
            .client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            // S3 delete is idempotent (a missing object is not an error).
            Ok(_) => Ok(()),
            Err(e) => {
                tracing::warn!(error = %e, key = %key, "S3 delete failed");
                Err(StorageError::Unavailable)
            }
        }
    }

    async fn get(&self, key: &str) -> Result<(Vec<u8>, String), StorageError> {
        self.safe_key(key)?;
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                if matches!(
                    e.as_service_error(),
                    Some(aws_sdk_s3::operation::get_object::GetObjectError::NoSuchKey(_))
                ) {
                    StorageError::NotFound
                } else {
                    tracing::warn!(error = %e, key = %key, "S3 get failed");
                    StorageError::Unavailable
                }
            })?;
        let content_type = resp
            .content_type()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let bytes = resp
            .body
            .collect()
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, key = %key, "S3 get body failed");
                StorageError::Unavailable
            })?
            .into_bytes()
            .to_vec();
        Ok((bytes, content_type))
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

/// Shares a single [`ObjectStorage`] across a `Box<dyn ObjectStorage>` consumer
/// (photo service) and the app's `Arc<dyn ObjectStorage>` state, without `Clone`
/// on the trait object.
#[derive(Clone)]
pub struct SharedObjectStorage(Arc<dyn ObjectStorage>);

impl SharedObjectStorage {
    pub fn new(inner: Arc<dyn ObjectStorage>) -> Self {
        Self(inner)
    }
}

#[async_trait]
impl ObjectStorage for SharedObjectStorage {
    async fn put(&self, req: PutObject<'_>) -> Result<String, StorageError> {
        self.0.put(req).await
    }

    fn presigned_get(&self, key: &str, ttl: Duration) -> Result<String, StorageError> {
        self.0.presigned_get(key, ttl)
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        self.0.delete(key).await
    }

    async fn get(&self, key: &str) -> Result<(Vec<u8>, String), StorageError> {
        self.0.get(key).await
    }

    fn verify_get(&self, key: &str, exp: u64, sig: &str) -> bool {
        self.0.verify_get(key, exp, sig)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> S3ObjectStorage {
        S3ObjectStorage::new(
            Some("http://localhost:9000".to_string()),
            DEFAULT_S3_REGION.to_string(),
            "bikenest".to_string(),
            "minioadmin".to_string(),
            "minioadmin".to_string(),
            b"secret".to_vec(),
        )
    }

    #[test]
    fn signed_media_url_verifies_and_detects_tampering() {
        let s = store();
        let url = s.presigned_get("seed/x.jpg", Duration::from_secs(60)).unwrap();
        assert!(url.starts_with("/media/seed/x.jpg?"), "uses the /media route: {url}");
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
    fn rejects_bad_keys() {
        let s = store();
        assert!(s.presigned_get("a?b.jpg", Duration::from_secs(1)).is_err());
        assert!(s.presigned_get("", Duration::from_secs(1)).is_err());
    }

    #[test]
    fn from_env_defaults_to_local_minio() {
        assert_eq!(DEFAULT_S3_BUCKET, "bikenest");
        assert_eq!(DEFAULT_S3_REGION, "us-east-1");
        assert_eq!(MEDIA_BASE_PATH, "/media");
    }
}
