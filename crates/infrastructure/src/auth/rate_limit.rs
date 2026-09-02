//! In-memory rate limiter (**Ledger #6**). Sliding-window counter per key.
//! Fine for single-instance dev (M2 auth endpoints); a shared/Redis-backed
//! store replaces it before multi-instance deployment (M7).

use async_trait::async_trait;
use bikenest_application::{RateLimitError, RateLimiter};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

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
