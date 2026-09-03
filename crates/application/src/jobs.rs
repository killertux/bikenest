//! Background job abstraction (plans/m9-background-jobs.md).
//!
//! Pure ports/vals only — no SQL. Infrastructure implements [`JobHandler`]
//! (concrete handlers that capture the ports they need) and the SQL job
//! repository/worker. A job is a durable unit of work with a retry budget,
//! claimed by an in-process worker, and either executed to `succeeded` or
//! dead-lettered to `failed`.

use async_trait::async_trait;
use serde_json::Value;

/// A job payload: the raw versioned JSON body for one job kind. Handlers
/// deserialize it into a typed struct (`serde_json::from_value(payload.clone())`).
/// Adding a new kind is a payload shape + a handler — no schema change.
pub type JobPayload = Value;

/// Discriminator constants for the built-in job kinds.
pub const JOB_RETENTION: &str = "retention";
pub const JOB_JOBS_GC: &str = "jobs.gc";

/// A job-run failure. `Failed` is transient (the worker retries it if the
/// attempt budget remains); `Permanent` skips retries and dead-letters now.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("job failed: {0}")]
    Failed(String),
    #[error("job failed permanently (no retry): {0}")]
    Permanent(String),
}

/// A handler for one job `kind`.
///
/// Implementations **must tolerate at-least-once execution**: a worker crash
/// leaves a job leasable, and once the lease expires another worker re-claims
/// and re-runs it. Keep handlers idempotent (a repeated `DELETE WHERE expires_at
/// < now()` or an upsert is safe; a side-effecting send is not).
#[async_trait]
pub trait JobHandler: Send + Sync {
    fn kind(&self) -> &'static str;

    /// Runs the job. `Ok(())` = success; `Err(JobError)` = failure.
    async fn run(&self, payload: &JobPayload) -> Result<(), JobError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_kinds_are_stable() {
        assert_eq!(JOB_RETENTION, "retention");
        assert_eq!(JOB_JOBS_GC, "jobs.gc");
    }
}
