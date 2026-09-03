//! Integration test for the S3-compatible [`S3ObjectStorage`] (**Ledger #7**),
//! against a real MinIO server (or any S3-compatible target).
//!
//! Gated on env so `cargo test` stays green without a storage service. Defaults
//! target a local MinIO (`docker compose up -d minio minio-init`):
//! - `S3_TEST_ENDPOINT` (default `http://localhost:9000`)
//! - `S3_TEST_BUCKET` (default `bikenest`)
//! - `S3_TEST_ACCESS_KEY_ID` / `S3_TEST_SECRET_ACCESS_KEY` (default `minioadmin`)
//!
//! Run it live with:
//! ```bash
//! cargo test -p bikenest-infrastructure --test s3_object_storage -- --nocapture
//! ```

use bikenest_application::ObjectStorage;
use bikenest_infrastructure::S3ObjectStorage;
use std::time::Duration;

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_string())
}

fn store() -> S3ObjectStorage {
    S3ObjectStorage::new(
        Some(env_or("S3_TEST_ENDPOINT", "http://localhost:9000")),
        env_or("S3_TEST_REGION", "us-east-1"),
        env_or("S3_TEST_BUCKET", "bikenest"),
        env_or("S3_TEST_ACCESS_KEY_ID", "minioadmin"),
        env_or("S3_TEST_SECRET_ACCESS_KEY", "minioadmin"),
        b"test-secret".to_vec(),
    )
}

#[tokio::test]
async fn put_get_delete_round_trip() {
    let s = store();
    let key = format!("test/s3-roundtrip/{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    let bytes = b"hello s3";

    let stored = s
        .put(bikenest_application::PutObject {
            key: key.clone(),
            bytes,
            content_type: "image/jpeg".to_string(),
        })
        .await;
    let Ok(k) = stored else {
        eprintln!("S3 put failed (is MinIO up? `docker compose up -d minio minio-init`): {stored:?}");
        return;
    };
    assert_eq!(k, key);

    let (got, ct) = s.get(&key).await.expect("get should succeed");
    assert_eq!(got, bytes);
    assert_eq!(ct, "image/jpeg");

    // predigned URL is a signed /media URL that verifies.
    let url = s.presigned_get(&key, Duration::from_secs(60)).unwrap();
    assert!(url.starts_with("/media/"), "signed /media url: {url}");
    assert!(url.contains("exp=") && url.contains("sig="), "carries exp+sig: {url}");
    let (_, qs) = url.split_once('?').unwrap();
    let mut exp = 0u64;
    let mut sig = String::new();
    for kv in qs.split('&') {
        if let Some((k, v)) = kv.split_once('=') {
            if k == "exp" { exp = v.parse().unwrap(); }
            if k == "sig" { sig = v.to_string(); }
        }
    }
    assert!(s.verify_get(&key, exp, &sig), "signed url verifies");

    s.delete(&key).await.expect("delete should succeed");
    assert!(matches!(s.get(&key).await, Err(bikenest_application::StorageError::NotFound)), "deleted object is gone");
}
