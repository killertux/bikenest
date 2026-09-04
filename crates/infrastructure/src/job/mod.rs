//! PostgreSQL-backed background job queue (plans/m9-background-jobs.md).
//!
//! A durable one-shot + recurring job table claimed by in-process workers.
//! All timestamps are `TIMESTAMPTZ` (UTC); there is no per-job timezone.

pub mod email;
pub mod registry;
pub mod repo;
pub mod schedule;
pub mod worker;

pub use email::SendEmailHandler;
pub use registry::{
    JobRegistry, JobServices, JobsGcHandler, RecurringKind, RetentionJobHandler, job_services,
};
pub use repo::{ClaimedJob, JobRepoError, SqlxJobRepository};
pub use schedule::{backoff_ms, next_run_at};
pub use worker::Worker;
