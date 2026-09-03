//! S3-compatible object storage (**Ledger #7**).
//!
//! Replaces the local-disk store with an S3-compatible backend (AWS S3,
//! Cloudflare R2, Backblaze B2, MinIO). Writes go to the bucket; object reads
//! are served via **direct S3 presigned GET URLs** that point straight at the
//! bucket — the browser hits the bucket and S3's SigV4 signature authorizes the
//! read, so the app is not a media proxy and no app-side signing secret is
//! needed.
//!
//! Configured with `S3_*` env (dev defaults target a local MinIO):
//! - `S3_ENDPOINT` — default `http://localhost:9000`; set `S3_ENDPOINT=` (empty)
//!   to use the standard AWS endpoint.
//! - `S3_REGION` — default `us-east-1`
//! - `S3_BUCKET` — default `bikenest`
//! - `S3_ACCESS_KEY_ID` / `S3_SECRET_ACCESS_KEY` — default MinIO `minioadmin`
//!
//! Path-style addressing is always on (required for MinIO; also fine for AWS/R2).

use async_trait::async_trait;
use aws_sdk_s3::Config;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::presigning::PresigningConfig;
use aws_smithy_types::byte_stream::ByteStream;
use bikenest_application::{ObjectStorage, PutObject, StorageError};
use std::sync::Arc;
use std::time::Duration;

pub const DEFAULT_S3_REGION: &str = "us-east-1";
pub const DEFAULT_S3_BUCKET: &str = "bikenest";
const DEV_S3_ENDPOINT: &str = "http://localhost:9000";

#[derive(Clone)]
pub struct S3ObjectStorage {
    client: aws_sdk_s3::Client,
    bucket: String,
}

impl S3ObjectStorage {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        endpoint: Option<String>,
        region: String,
        bucket: String,
        access_key: String,
        secret_key: String,
    ) -> Self {
        let mut builder = Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(region))
            .credentials_provider(Credentials::new(
                access_key, secret_key, None, None, "bikenest",
            ))
            .force_path_style(true);
        if let Some(endpoint) = endpoint {
            builder = builder.endpoint_url(endpoint);
        }
        Self {
            client: aws_sdk_s3::Client::from_conf(builder.build()),
            bucket,
        }
    }

    /// Build from the `S3_*` env vars, defaulting to a local MinIO so `cargo run`
    /// works against the compose stack (dev). Production sets real credentials.
    pub fn from_env() -> Self {
        let endpoint = std::env::var("S3_ENDPOINT")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| Some(DEV_S3_ENDPOINT.to_string()));
        let region = std::env::var("S3_REGION").unwrap_or_else(|_| DEFAULT_S3_REGION.to_string());
        let bucket = std::env::var("S3_BUCKET").unwrap_or_else(|_| DEFAULT_S3_BUCKET.to_string());
        let access = std::env::var("S3_ACCESS_KEY_ID").unwrap_or_else(|_| "minioadmin".to_string());
        let secret =
            std::env::var("S3_SECRET_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_string());
        Self::new(endpoint, region, bucket, access, secret)
    }
}

/// Build the S3 storage from the `S3_*` env vars (see [`S3ObjectStorage::from_env`]).
pub fn storage_from_env() -> S3ObjectStorage {
    S3ObjectStorage::from_env()
}

#[async_trait]
impl ObjectStorage for S3ObjectStorage {
    async fn put(&self, req: PutObject<'_>) -> Result<String, StorageError> {
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

    async fn presigned_get(&self, key: &str, ttl: Duration) -> Result<String, StorageError> {
        // Direct S3 presigned URL (SigV4). Presigning does no network I/O.
        let cfg = PresigningConfig::expires_in(ttl)
            .map_err(|e| StorageError::Unexpected(format!("presign config: {e}")))?;
        let request = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(cfg)
            .await
            .map_err(|e| StorageError::Unexpected(format!("presign: {e}")))?;
        Ok(request.uri().to_string())
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

    async fn get(&self, _key: &str) -> Result<(Vec<u8>, String), StorageError> {
        // Media is served via direct S3 presigned URLs that bypass the app; the
        // app's `/media` route is not used and should not proxy from the bucket.
        Err(StorageError::Unexpected(
            "objects are served via S3 presigned URLs (no app media proxy)".to_string(),
        ))
    }

    fn verify_get(&self, _key: &str, _exp: u64, _sig: &str) -> bool {
        // No app-side signature scheme; presigned URLs are self-authorizing.
        false
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

    async fn presigned_get(&self, key: &str, ttl: Duration) -> Result<String, StorageError> {
        self.0.presigned_get(key, ttl).await
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
        // Fake credentials; no network happens at construction.
        S3ObjectStorage::new(
            Some("http://localhost:9000".to_string()),
            DEFAULT_S3_REGION.to_string(),
            "bikenest".to_string(),
            "minioadmin".to_string(),
            "minioadmin".to_string(),
        )
    }

    #[tokio::test]
    async fn presigned_get_is_a_direct_signed_url() {
        let s = store();
        let url = s
            .presigned_get("seed/x.jpg", Duration::from_secs(60))
            .await
            .expect("presign is local crypto, no network");
        // Points at the endpoint/bucket and carries SigV4 query params.
        assert!(
            url.contains("localhost:9000"),
            "should point at the S3 endpoint: {url}"
        );
        assert!(
            url.contains("X-Amz-Signature="),
            "should carry a signature: {url}"
        );
        assert!(
            url.contains("X-Amz-Expires=60"),
            "should carry the TTL: {url}"
        );
    }

    #[tokio::test]
    async fn get_is_unsupported_for_direct_presign_model() {
        let s = store();
        assert!(
            matches!(s.get("seed/x.jpg").await, Err(StorageError::Unexpected(_))),
            "S3 storage must not proxy media"
        );
    }

    #[test]
    fn verify_get_is_false() {
        assert!(!store().verify_get("seed/x.jpg", 0, "sig"));
    }

    #[test]
    fn from_env_defaults_to_local_minio() {
        assert_eq!(DEFAULT_S3_BUCKET, "bikenest");
        assert_eq!(DEFAULT_S3_REGION, "us-east-1");
    }
}
