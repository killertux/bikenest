//! S3-compatible object storage (**Ledger #7**).
//!
//! Replaces the local-disk store with an S3-compatible backend (AWS S3,
//! Cloudflare R2, Backblaze B2, MinIO). Writes go to the bucket; object reads
//! are served via **direct S3 presigned GET URLs** that point straight at the
//! bucket — the browser hits the bucket and S3's SigV4 signature authorizes the
//! read, so the app is not a media proxy and no app-side signing secret is
//! needed.
//!
//! The `S3_*` block of [`crate::config::Config`] supplies endpoint, region,
//! bucket and credentials; development defaults target the compose MinIO while
//! production gets no defaults at all.
//!
//! Path-style addressing is always on (required for MinIO; also fine for AWS/R2).

use crate::config::S3Config;
use async_trait::async_trait;
use aws_sdk_s3::Config;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::presigning::PresigningConfig;
use aws_smithy_types::byte_stream::ByteStream;
use bikenest_application::{ObjectStorage, PutObject, StorageError};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct S3ObjectStorage {
    client: aws_sdk_s3::Client,
    bucket: String,
}

impl S3ObjectStorage {
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

    /// Build from the parsed `S3_*` configuration. Construction is local (no
    /// network), so this cannot fail.
    pub fn from_config(config: &S3Config) -> Self {
        Self::new(
            config.endpoint.clone(),
            config.region.clone(),
            config.bucket.clone(),
            config.access_key_id.clone(),
            config.secret_access_key.clone(),
        )
    }
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

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                if e.as_service_error().is_some_and(|s| s.is_not_found()) {
                    return Ok(false);
                }
                tracing::warn!(error = %e, key = %key, "S3 head_object failed");
                Err(StorageError::Unavailable)
            }
        }
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

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        self.0.exists(key).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn store() -> S3ObjectStorage {
        // Fake credentials; no network happens at construction.
        S3ObjectStorage::from_config(&Config::for_tests("postgres://localhost/x").storage)
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

    #[test]
    fn development_config_targets_the_compose_minio() {
        let s3 = Config::for_tests("postgres://localhost/x").storage;
        assert_eq!(s3.bucket, crate::config::DEFAULT_S3_BUCKET);
        assert_eq!(s3.region, crate::config::DEFAULT_S3_REGION);
        assert_eq!(s3.endpoint.as_deref(), Some("http://localhost:9000"));
    }
}
