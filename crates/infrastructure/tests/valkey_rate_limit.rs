//! Integration tests for the ValKey rate limiter (****).
//!
//! These connect to a real ValKey/Redis server, so they are **gated on env**:
//! - `VALKEY_TEST_URL` — single-node URL (e.g. `valkey://localhost:6379`).
//! - `VALKEY_TEST_CLUSTER_URLS` — comma-separated cluster node URLs.
//!
//! When the variable is unset the test is skipped (logged, still passes), so the
//! default `cargo test` run (no ValKey) stays green. Run them against a live
//! backend with:
//!
//! ```bash
//! VALKEY_TEST_URL=valkey://localhost:6379 \
//! VALKEY_TEST_CLUSTER_URLS=valkey://localhost:7001,valkey://localhost:7002,valkey://localhost:7003 \
//! cargo test -p bikesnest-infrastructure --test valkey_rate_limit -- --nocapture
//! ```

use bikesnest_application::RateLimiter;
use bikesnest_infrastructure::ValKeyRateLimiter;
use std::time::Duration;

/// A per-run unique key prefix so tests don't accumulate across re-runs
/// (a ValKey is a shared, persistent store).
fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[tokio::test]
async fn single_node_sliding_window() {
    let Some(url) = std::env::var("VALKEY_TEST_URL").ok() else {
        eprintln!("VALKEY_TEST_URL not set; skipping single-node ValKey test");
        return;
    };
    let rl = ValKeyRateLimiter::single(url, false).expect("valid single-node config");
    let key = format!("test:rl:single:{}", unique_suffix());
    let limit = 3u32;
    let window = Duration::from_secs(60);

    let mut results = Vec::new();
    for _ in 0..(limit + 2) {
        results.push(rl.check(&key, limit, window).await.expect("no error"));
    }

    // First `limit` calls allowed, then denied.
    assert_eq!(
        results,
        [true, true, true, false, false],
        "sliding window should allow limit then deny"
    );

    // An unrelated key is independent.
    let fresh = format!("{key}:fresh");
    assert!(
        rl.check(&fresh, limit, window).await.expect("no error"),
        "a fresh key should be allowed"
    );
}

#[tokio::test]
async fn cluster_sliding_window() {
    let Some(nodes) = std::env::var("VALKEY_TEST_CLUSTER_URLS").ok() else {
        eprintln!("VALKEY_TEST_CLUSTER_URLS not set; skipping cluster ValKey test");
        return;
    };
    let urls: Vec<String> = nodes
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let rl = ValKeyRateLimiter::cluster(urls.clone(), false).expect("valid cluster config");
    let key = format!("test:rl:cluster:{}", unique_suffix());
    let limit = 2u32;
    let window = Duration::from_secs(60);

    let mut results = Vec::new();
    for _ in 0..(limit + 2) {
        results.push(rl.check(&key, limit, window).await.expect("no error"));
    }
    assert_eq!(
        results,
        [true, true, false, false],
        "cluster should enforce the same sliding window"
    );

    // A second client pointing at the same cluster agrees on the shared counter
    // (i.e. the limit aggregates across processes, not per client).
    let other = ValKeyRateLimiter::cluster(urls, false).expect("valid cluster config");
    for _ in 0..(limit + 1) {
        assert!(
            !other.check(&key, limit, window).await.expect("no error"),
            "second client should see the shared counter"
        );
    }
}
