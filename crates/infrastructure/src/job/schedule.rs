//! Pure scheduling helpers (plans/m9-background-jobs.md). No DB.
//!
//! All job timestamps are UTC. `next_run_at` derives a recurring job's next run
//! time from its `schedule` JSONB: `{"every_seconds":N}` → `now + Ns`;
//! `{"cron":"…"}` → the next tick after `now` evaluated in **UTC** (`cron::Schedule::after`).
//! An invalid/unsupported schedule is a permanent error (the worker dead-letters).

use bikesnest_application::JobError;
use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use serde_json::Value;
use std::str::FromStr;

/// The next run time for a schedule, in UTC. `None` for a one-shot job
/// (`schedule` is `NULL`/`null`). `Err(Permanent)` for an invalid schedule.
///
/// Accepted `schedule` shapes (recurring): `{"every_seconds": N}` → `now + Ns`;
/// `{"cron": "…"}` → the next tick after `now` in UTC. The `cron` crate is
/// seconds-based (6 or 7 fields), so a common Unix 5-field expression
/// (`min hour dom mon dow`) is normalized to 6 fields (seconds forced to 0),
/// i.e. minute resolution. All times are UTC.
pub fn next_run_at(
    schedule: Option<&Value>,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, JobError> {
    let Some(s) = schedule else { return Ok(None) };
    if s.is_null() {
        return Ok(None);
    }
    if !s.is_object() {
        return Err(JobError::Permanent(format!(
            "schedule must be an object, got {s}"
        )));
    }
    if let Some(secs) = s.get("every_seconds").and_then(|v| v.as_i64()) {
        if secs <= 0 {
            return Err(JobError::Permanent(format!(
                "every_seconds must be positive, got {secs}"
            )));
        }
        return Ok(Some(now + Duration::seconds(secs)));
    }
    if let Some(expr) = s.get("cron").and_then(|v| v.as_str()) {
        let normalized = normalize_cron(expr);
        // `after(&now)` evaluates the next tick after `now` in UTC (deterministic).
        let parsed = cron::Schedule::from_str(&normalized)
            .map_err(|e| JobError::Permanent(format!("invalid cron expression {expr:?}: {e}")))?;
        let next = parsed
            .after(&now)
            .next()
            .ok_or_else(|| JobError::Permanent(format!("cron {expr:?} yields no future run")))?;
        return Ok(Some(next));
    }
    Err(JobError::Permanent(format!("unsupported schedule: {s}")))
}

/// The `cron` crate is seconds-based (6/7 fields); accept the common Unix 5-field
/// form by prepending a `0` seconds field (minute resolution).
fn normalize_cron(expr: &str) -> String {
    if expr.split_whitespace().count() == 5 {
        format!("0 {expr}")
    } else {
        expr.to_string()
    }
}

/// Retry delay for the 1-based `attempt`: `base * 2^(attempt-1)` + jitter in
/// `[0, base)`. Capped by `u64` so a large attempt count cannot overflow.
pub fn backoff_ms(attempt: i32, base_ms: u64) -> u64 {
    let exp = (attempt - 1).max(0) as u32;
    let exponential = base_ms.saturating_mul(1u64 << exp.min(63));
    let jitter = rand::thread_rng().gen_range(0..base_ms.max(1));
    exponential.saturating_add(jitter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 2025-01-01 00:00:00 UTC plus a number of minutes.
    fn base_plus(minutes: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_735_689_600, 0).unwrap() + Duration::minutes(minutes)
    }

    #[test]
    fn null_schedule_is_oneshot_but_empty_object_is_permanent() {
        assert_eq!(next_run_at(None, base_plus(0)).unwrap(), None);
        assert_eq!(next_run_at(Some(&Value::Null), base_plus(0)).unwrap(), None);
        // An object with no recognized scheduling field is a misconfiguration.
        let err = next_run_at(Some(&json!({})), base_plus(0)).unwrap_err();
        assert!(matches!(err, JobError::Permanent(_)));
    }

    #[test]
    fn every_seconds_offsets_from_now() {
        let now = base_plus(0);
        let next = next_run_at(Some(&json!({ "every_seconds": 60 })), now).unwrap();
        assert_eq!(next.unwrap() - now, Duration::seconds(60));
    }

    #[test]
    fn every_seconds_zero_is_permanent() {
        let err = next_run_at(Some(&json!({ "every_seconds": 0 })), base_plus(0)).unwrap_err();
        assert!(matches!(err, JobError::Permanent(_)));
    }

    #[test]
    fn cron_next_tick_is_utc() {
        // "minute 3 of hour 1" every day: at 02:00 UTC the next run is 03:00 UTC same day.
        let now = base_plus(120); // 02:00 UTC
        let next = next_run_at(Some(&json!({ "cron": "0 3 * * *" })), now)
            .unwrap()
            .expect("has a next tick");
        assert_eq!(next, base_plus(180)); // 03:00 UTC
    }

    #[test]
    fn cron_invalid_expression_is_permanent() {
        let err = next_run_at(Some(&json!({ "cron": "not a cron" })), base_plus(0)).unwrap_err();
        assert!(matches!(err, JobError::Permanent(_)));
    }

    #[test]
    fn unsupported_schedule_is_permanent() {
        let err = next_run_at(Some(&json!({ "bogus": 1 })), base_plus(0)).unwrap_err();
        assert!(matches!(err, JobError::Permanent(_)));
    }

    #[test]
    fn backoff_is_exponential_and_jittered() {
        let base = 1000u64;
        // attempt 1 → base (1000..<2000), attempt 2 → 2*base (2000..<3000), etc.
        let a1 = backoff_ms(1, base);
        let a2 = backoff_ms(2, base);
        assert!((1000..2000).contains(&a1));
        assert!((2000..3000).contains(&a2));
        // Strictly non-decreasing with attempt.
        assert!(a2 > a1);
    }
}
