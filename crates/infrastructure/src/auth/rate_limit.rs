//! Rate-limit stores (****). Both implementations implement the
//! [`RateLimiter`] port.
//!
//! - [`InMemoryRateLimiter`] — per-process sliding-window counter; fine for
//!   single-instance dev (M2).
//! - [`ValKeyRateLimiter`] — sliding-window counter stored in a ValKey/Redis
//!   sorted set via an atomic Lua script. Limits aggregate across instances and
//!   survive restarts. Supports a single node (`VALKEY_URL`) or a cluster
//!   (`VALKEY_CLUSTER_URLS`).
//! - [`rate_limiter_from_config`] wires whichever backend the parsed
//!   configuration selected; a ValKey backend that fails to connect is a startup
//!   error rather than a silent downgrade to the per-process limiter.
//!
//! ## Failure mode
//! The application maps *any* [`RateLimitError`] to "RateLimited" (fail closed).
//! A ValKey outage would therefore 429 every rate-limited endpoint, taking the
//! site down. [`ValKeyRateLimiter`] therefore **fails open by default**: on a
//! connectivity/CAS error it logs a `warn!` and returns `Ok(true)` (allow), so a
//! ValKey outage degrades protection without an outage. Set `RATE_LIMIT_FAIL_OPEN=false`
//! to fail closed instead (stricter, but a ValKey outage 429s auth/photo/moderation).

use crate::config::{ConfigError, RateLimiterBackend, RateLimiterConfig};

use async_trait::async_trait;
use bikesnest_application::{RateLimitError, RateLimiter};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// In-memory
// ---------------------------------------------------------------------------

/// Hard ceiling on the number of live buckets. Rate-limit keys embed
/// caller-controlled strings (client address, e-mail), so a map that only ever
/// grows is a memory-exhaustion vector: one attacker with a header they choose
/// can mint a bucket per request. Dropping a bucket only *forgets* past events,
/// so the worst case of an eviction is a few extra allowed requests — never a
/// false 429.
pub const MAX_BUCKETS: usize = 100_000;

/// How many *new* keys may be added between sweeps. Sweeping is O(live keys),
/// so it is amortised over this many insertions rather than run per request.
const SWEEP_EVERY_NEW_KEYS: u32 = 512;

/// One key's sliding window plus when it was last touched (eviction order) and
/// the window it was last checked with (so a sweep triggered by a short-window
/// key cannot drop a long-window bucket that is still live).
struct Bucket {
    events: VecDeque<Instant>,
    last_seen: Instant,
    window: Duration,
}

#[derive(Default)]
struct Buckets {
    map: HashMap<String, Bucket>,
    /// New keys added since the last sweep.
    since_sweep: u32,
}

#[derive(Default)]
pub struct InMemoryRateLimiter {
    buckets: Mutex<Buckets>,
}

impl InMemoryRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of live buckets. Exposed so the tests can assert the map stays
    /// bounded and that expired buckets are actually reclaimed.
    pub fn bucket_count(&self) -> usize {
        self.buckets.lock().map(|b| b.map.len()).unwrap_or(0)
    }

    /// Drop every bucket whose whole window has elapsed (its deque would trim
    /// to empty), then — if the map is still at the cap — the
    /// least-recently-touched keys until there is headroom again.
    fn sweep(buckets: &mut Buckets, now: Instant) {
        buckets.since_sweep = 0;
        buckets
            .map
            .retain(|_, b| now.duration_since(b.last_seen) < b.window);
        if buckets.map.len() < MAX_BUCKETS {
            return;
        }
        let target = MAX_BUCKETS - MAX_BUCKETS / 10;
        let excess = buckets.map.len() - target;
        let mut ages: Vec<(Instant, String)> = buckets
            .map
            .iter()
            .map(|(k, b)| (b.last_seen, k.clone()))
            .collect();
        ages.sort_unstable_by_key(|(t, _)| *t);
        for (_, key) in ages.into_iter().take(excess) {
            buckets.map.remove(&key);
        }
    }
}

#[async_trait]
impl RateLimiter for InMemoryRateLimiter {
    async fn check(&self, key: &str, limit: u32, window: Duration) -> Result<bool, RateLimitError> {
        let now = Instant::now();
        let mut buckets = self
            .buckets
            .lock()
            .map_err(|_| RateLimitError::Unavailable)?;

        // Only a *new* key can grow the map, so that is the only place a sweep
        // can be needed.
        if !buckets.map.contains_key(key) {
            buckets.since_sweep += 1;
            if buckets.since_sweep >= SWEEP_EVERY_NEW_KEYS || buckets.map.len() >= MAX_BUCKETS {
                Self::sweep(&mut buckets, now);
            }
        }

        let bucket = buckets
            .map
            .entry(key.to_string())
            .or_insert_with(|| Bucket {
                events: VecDeque::new(),
                last_seen: now,
                window,
            });
        bucket.last_seen = now;
        bucket.window = window;

        // Drop events that have fallen outside the window.
        while let Some(front) = bucket.events.front() {
            if now.duration_since(*front) >= window {
                bucket.events.pop_front();
            } else {
                break;
            }
        }

        // Over the limit → deny. Otherwise record this event and allow. (A
        // fresh bucket must still be recorded, or the counter never accrues and
        // the limit never trips.)
        if bucket.events.len() >= limit as usize {
            return Ok(false);
        }
        bucket.events.push_back(now);
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// ValKey / Redis
// ---------------------------------------------------------------------------

/// Atomic sliding-window rate limit against a ValKey sorted set.
///
/// `KEYS[1]` is the rate bucket (a ZSET of event timestamps). Old events are
/// trimmed, `ZCARD` counts the current-window hits, and the event is recorded
/// (with a unique member) only if the count is under the limit. Returns `1`
/// (allow) or `0` (over limit).
const RATE_LIMIT_LUA: &str = r#"
-- ARGV[1] = now (ms), ARGV[2] = window (ms), ARGV[3] = limit, ARGV[4] = member.
local now = tonumber(ARGV[1])
local window = tonumber(ARGV[2])
local limit = tonumber(ARGV[3])
redis.call('ZREMRANGEBYSCORE', KEYS[1], 0, now - window)
local count = redis.call('ZCARD', KEYS[1])
if count >= limit then
    return 0
end
redis.call('ZADD', KEYS[1], now, ARGV[4])
redis.call('PEXPIRE', KEYS[1], window)
return 1
"#;

/// Connection target for a [`ValKeyRateLimiter`]. Both variants are cheap to
/// clone (they share the underlying connection pool / cluster state).
#[derive(Clone)]
enum ValKeyConn {
    /// Single node (`VALKEY_URL`) via a multiplexed connection manager.
    Single(redis::aio::ConnectionManager),
    /// Cluster (`VALKEY_CLUSTER_URLS`) via the async cluster connection.
    Cluster(redis::cluster_async::ClusterConnection),
}

/// Where a [`ValKeyRateLimiter`] points. Parsed from env and validated at
/// construction; the connection itself is established lazily on first use.
#[derive(Clone)]
enum ValKeyConfig {
    Single(String),
    Cluster(Vec<String>),
}

/// Shared, cluster-compatible ValKey rate limiter. The connection is built lazily
/// on the first `check` (so it can be constructed from the synchronous
/// `app_router_with` wiring path), then shared across calls via cheap clones.
pub struct ValKeyRateLimiter {
    config: ValKeyConfig,
    conn: tokio::sync::Mutex<Option<ValKeyConn>>,
    /// When true (default), a ValKey error allows the request instead of 429ing.
    fail_open: bool,
}

impl ValKeyRateLimiter {
    /// A single-node limiter (`VALKEY_URL`, e.g. `valkey://localhost:6379`).
    pub fn single(url: impl Into<String>, fail_open: bool) -> Result<Self, RateLimitError> {
        let url = url.into();
        // Validate the URL now (does not connect) so configuration errors surface
        // at wiring time rather than on the first request.
        redis::Client::open(url.clone())
            .map_err(|e| RateLimitError::Unexpected(format!("invalid VALKEY_URL: {e}")))?;
        Ok(Self {
            config: ValKeyConfig::Single(url),
            conn: tokio::sync::Mutex::new(None),
            fail_open,
        })
    }

    /// A cluster limiter (`VALKEY_CLUSTER_URLS`, a comma-separated list of node
    /// URLs). The client auto-discovers the cluster topology.
    pub fn cluster(urls: Vec<String>, fail_open: bool) -> Result<Self, RateLimitError> {
        if urls.is_empty() {
            return Err(RateLimitError::Unexpected(
                "VALKEY_CLUSTER_URLS is empty".to_string(),
            ));
        }
        // Validate node URLs now (ClusterClient::new does not connect).
        redis::cluster::ClusterClient::new(urls.clone())
            .map_err(|e| RateLimitError::Unexpected(format!("invalid VALKEY_CLUSTER_URLS: {e}")))?;
        Ok(Self {
            config: ValKeyConfig::Cluster(urls),
            conn: tokio::sync::Mutex::new(None),
            fail_open,
        })
    }

    /// Establish the connection on first use (thread-safe, one-time).
    async fn conn(&self) -> Result<ValKeyConn, RateLimitError> {
        let mut guard = self.conn.lock().await;
        if let Some(conn) = &*guard {
            return Ok(conn.clone());
        }
        let conn = self.connect().await?;
        *guard = Some(conn.clone());
        Ok(conn)
    }

    async fn connect(&self) -> Result<ValKeyConn, RateLimitError> {
        match &self.config {
            ValKeyConfig::Single(url) => {
                let client = redis::Client::open(url.clone())
                    .map_err(|e| RateLimitError::Unexpected(format!("valkey connect: {e}")))?;
                let manager = redis::aio::ConnectionManager::new(client)
                    .await
                    .map_err(|e| RateLimitError::Unexpected(format!("valkey connect: {e}")))?;
                Ok(ValKeyConn::Single(manager))
            }
            ValKeyConfig::Cluster(urls) => {
                let client = redis::cluster::ClusterClient::new(urls.clone()).map_err(|e| {
                    RateLimitError::Unexpected(format!("valkey cluster connect: {e}"))
                })?;
                let conn = client.get_async_connection().await.map_err(|e| {
                    RateLimitError::Unexpected(format!("valkey cluster connect: {e}"))
                })?;
                Ok(ValKeyConn::Cluster(conn))
            }
        }
    }
}

#[async_trait]
impl RateLimiter for ValKeyRateLimiter {
    async fn check(&self, key: &str, limit: u32, window: Duration) -> Result<bool, RateLimitError> {
        let mut conn = match self.conn().await {
            Ok(conn) => conn,
            Err(e) => {
                tracing::warn!(error = %e, %key, "rate limiter unavailable; failing open");
                return Ok(true);
            }
        };

        let now_ms = now_millis();
        // Unique ZSET member per event (two events in the same millisecond must
        // not collapse to one member, and members must not collide across app
        // instances — hence a random suffix).
        let member = format!("{now_ms}:{:016x}", rand::random::<u64>());

        let result = match &mut conn {
            ValKeyConn::Single(c) => run_eval(c, key, limit, window, now_ms, &member).await,
            ValKeyConn::Cluster(c) => run_eval(c, key, limit, window, now_ms, &member).await,
        };

        match result {
            Ok(1) => Ok(true),
            Ok(_) => Ok(false),
            Err(e) => {
                if self.fail_open {
                    tracing::warn!(error = %e, %key, "rate limit check failed; failing open");
                    Ok(true)
                } else {
                    Err(RateLimitError::Unexpected(e.to_string()))
                }
            }
        }
    }
}

/// Run the sliding-window Lua script against either connection type.
///
/// Uses [`redis::Script`] (rather than a raw `EVAL`) so the KEYS are declared
/// via `.key()` and the command is routed to the owning cluster slot even in
/// cluster mode.
async fn run_eval<C: redis::aio::ConnectionLike>(
    conn: &mut C,
    key: &str,
    limit: u32,
    window: Duration,
    now_ms: u64,
    member: &str,
) -> redis::RedisResult<i64> {
    redis::Script::new(RATE_LIMIT_LUA)
        .key(key)
        .arg(now_ms)
        .arg(window.as_millis() as u64)
        .arg(limit)
        .arg(member)
        .invoke_async(conn)
        .await
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Selection + shared instance
// ---------------------------------------------------------------------------

/// Build the rate limiter the parsed configuration selected. A ValKey backend
/// that cannot be constructed is a startup error, not a silent downgrade to the
/// per-process in-memory limiter (which would multiply every limit by the
/// replica count).
pub fn rate_limiter_from_config(
    config: &RateLimiterConfig,
) -> Result<Box<dyn RateLimiter>, ConfigError> {
    let fail_open = config.fail_open;
    match &config.backend {
        RateLimiterBackend::InMemory => Ok(Box::new(InMemoryRateLimiter::new())),
        RateLimiterBackend::Valkey { url } => ValKeyRateLimiter::single(url.clone(), fail_open)
            .map(|l| Box::new(l) as Box<dyn RateLimiter>)
            .map_err(|e| ConfigError::invalid("VALKEY_URL", e.to_string())),
        RateLimiterBackend::ValkeyCluster { urls } => {
            ValKeyRateLimiter::cluster(urls.clone(), fail_open)
                .map(|l| Box::new(l) as Box<dyn RateLimiter>)
                .map_err(|e| ConfigError::invalid("VALKEY_CLUSTER_URLS", e.to_string()))
        }
    }
}

/// Shares a single [`RateLimiter`] across several service instances without
/// requiring `Clone` on the trait object: each `Box<dyn RateLimiter>` handed to a
/// service is a lightweight handle to the same underlying store.
#[derive(Clone)]
pub struct SharedRateLimiter(Arc<dyn RateLimiter>);

impl SharedRateLimiter {
    pub fn new(inner: Arc<dyn RateLimiter>) -> Self {
        Self(inner)
    }
}

#[async_trait]
impl RateLimiter for SharedRateLimiter {
    async fn check(&self, key: &str, limit: u32, window: Duration) -> Result<bool, RateLimitError> {
        self.0.check(key, limit, window).await
    }
}

// ---------------------------------------------------------------------------
// Tests (in-memory limiter only; the ValKey path has its own integration test)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_window_still_limits_and_then_reopens() {
        let rl = InMemoryRateLimiter::new();
        let window = Duration::from_millis(50);
        assert!(rl.check("k", 2, window).await.unwrap());
        assert!(rl.check("k", 2, window).await.unwrap());
        assert!(!rl.check("k", 2, window).await.unwrap(), "third is over");
        tokio::time::sleep(Duration::from_millis(70)).await;
        assert!(rl.check("k", 2, window).await.unwrap(), "window reopened");
    }

    /// A bucket whose window has fully elapsed is removed by the next sweep, so
    /// keys minted from attacker-controlled strings do not accumulate forever.
    #[tokio::test]
    async fn expired_buckets_are_reclaimed() {
        let rl = InMemoryRateLimiter::new();
        let short = Duration::from_millis(20);
        let long = Duration::from_secs(3600);
        rl.check("expires-quickly", 5, short).await.unwrap();
        assert_eq!(rl.bucket_count(), 1);
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Sweeps are amortised: one runs per SWEEP_EVERY_NEW_KEYS new keys.
        // These keys carry a long window, so only the expired one is dropped.
        for i in 0..SWEEP_EVERY_NEW_KEYS {
            rl.check(&format!("live-{i}"), 5, long).await.unwrap();
        }
        assert_eq!(
            rl.bucket_count(),
            SWEEP_EVERY_NEW_KEYS as usize,
            "the expired bucket must have been reclaimed, the live ones kept"
        );
    }

    /// Even with every key still inside its window, the map is capped: past the
    /// ceiling the least-recently-touched keys are evicted.
    #[tokio::test]
    async fn the_bucket_map_never_exceeds_the_cap() {
        let rl = InMemoryRateLimiter::new();
        let window = Duration::from_secs(3600);
        for i in 0..(MAX_BUCKETS + 2_000) {
            rl.check(&format!("key-{i}"), 5, window).await.unwrap();
        }
        assert!(
            rl.bucket_count() <= MAX_BUCKETS,
            "bucket map grew past the cap: {}",
            rl.bucket_count()
        );
        // Eviction leaves headroom rather than trimming one key at a time.
        assert!(rl.bucket_count() >= MAX_BUCKETS - MAX_BUCKETS / 10);
    }
}
