//! Rate-limit stores (**Ledger #6**). Both implementations implement the
//! [`RateLimiter`] port (§45).
//!
//! - [`InMemoryRateLimiter`] — per-process sliding-window counter; fine for
//!   single-instance dev (M2).
//! - [`ValKeyRateLimiter`] — sliding-window counter stored in a ValKey/Redis
//!   sorted set via an atomic Lua script. Limits aggregate across instances and
//!   survive restarts. Supports a single node (`VALKEY_URL`) or a cluster
//!   (`VALKEY_CLUSTER_URLS`).
//! - [`rate_limiter_from_env`] wires whichever backend the environment asks for,
//!   falling back to in-memory when no ValKey config is present.
//!
//! ## Failure mode
//! The application maps *any* [`RateLimitError`] to "RateLimited" (fail closed).
//! A ValKey outage would therefore 429 every rate-limited endpoint, taking the
//! site down. [`ValKeyRateLimiter`] therefore **fails open by default**: on a
//! connectivity/CAS error it logs a `warn!` and returns `Ok(true)` (allow), so a
//! ValKey outage degrades protection without an outage. Set `RATE_LIMIT_FAIL_OPEN=false`
//! to fail closed instead (stricter, but a ValKey outage 429s auth/photo/moderation).

use async_trait::async_trait;
use bikenest_application::{RateLimitError, RateLimiter};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// In-memory
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct InMemoryRateLimiter {
    buckets: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl InMemoryRateLimiter {
    pub fn new() -> Self {
        Self::default()
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
        let queue = buckets.entry(key.to_string()).or_default();

        // Drop events that have fallen outside the window.
        while let Some(front) = queue.front() {
            if now.duration_since(*front) >= window {
                queue.pop_front();
            } else {
                break;
            }
        }

        // Over the limit → deny. Otherwise record this event and allow. (A
        // fresh bucket must still be recorded, or the counter never accrues and
        // the limit never trips.)
        if queue.len() >= limit as usize {
            return Ok(false);
        }
        queue.push_back(now);
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
// Env selection + shared instance
// ---------------------------------------------------------------------------

/// Build the rate limiter the environment asks for.
///
/// - `VALKEY_CLUSTER_URLS` (comma-separated node URLs) → [`ValKeyRateLimiter::cluster`].
/// - `VALKEY_URL` → [`ValKeyRateLimiter::single`].
/// - Neither (or an invalid config) → [`InMemoryRateLimiter`] (dev fallback, so
///   `cargo run` and the test harness always work).
pub fn rate_limiter_from_env() -> Box<dyn RateLimiter> {
    let fail_open = fail_open_from_env();

    if let Ok(nodes) = std::env::var("VALKEY_CLUSTER_URLS").map(|s| {
        s.split(',')
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect::<Vec<_>>()
    }) && !nodes.is_empty()
    {
        match ValKeyRateLimiter::cluster(nodes, fail_open) {
            Ok(l) => return Box::new(l),
            Err(e) => {
                tracing::warn!(error = %e, "VALKEY_CLUSTER_URLS invalid; falling back to in-memory");
            }
        }
    }

    if let Ok(url) = std::env::var("VALKEY_URL")
        && !url.is_empty()
    {
        match ValKeyRateLimiter::single(url, fail_open) {
            Ok(l) => return Box::new(l),
            Err(e) => {
                tracing::warn!(error = %e, "VALKEY_URL invalid; falling back to in-memory");
            }
        }
    }

    Box::new(InMemoryRateLimiter::new())
}

fn fail_open_from_env() -> bool {
    std::env::var("RATE_LIMIT_FAIL_OPEN")
        .map(|v| {
            let v = v.to_ascii_lowercase();
            !(v == "false" || v == "0" || v == "no" || v == "off")
        })
        .unwrap_or(true)
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
