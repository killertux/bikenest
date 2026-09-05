//! SQL-backed job repository (plans/m9-background-jobs.md).
//!
//! The claim query uses `FOR UPDATE SKIP LOCKED` so multiple in-process workers
//! (or instances) pull disjoint jobs without blocking. Attempts are incremented
//! at claim so a crash-then-reclaim still burns budget and cannot loop forever.
//! A worker that dies mid-run leaves its job `state = 'running'` with a lease
//! that keeps counting down; once `lease_expires_at` is in the past, `claim`
//! treats that row exactly like a fresh `pending` one and reclaims it
//! (at-least-once). Every post-claim update (`finish_success`, `retry`, `fail`)
//! is scoped to `claimed_by = <the calling worker>`, so if the original
//! (zombie) worker wakes up after its lease has already been reassigned, its
//! stale write is a no-op instead of clobbering the new claim.

use crate::Db;
use bikesnest_application::JobPayload;
use chrono::{DateTime, Utc};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum JobRepoError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

/// A row claimed by a worker, ready to run.
#[derive(Debug, sqlx::FromRow)]
pub struct ClaimedJob {
    pub id: i64,
    pub kind: String,
    pub payload: JobPayload,
    /// Already incremented by the claim (the current attempt, 1-based).
    pub attempts: i32,
    pub max_attempts: i32,
    /// `{"every_seconds":N}` / `{"cron":"…"}` when recurring, else `NULL`.
    pub schedule: Option<Value>,
}

/// A job-queue handle bound to the application's PostgreSQL pool.
#[derive(Clone)]
pub struct SqlxJobRepository {
    db: Db,
}

impl SqlxJobRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Insert a job. `idempotency_key = Some(k)` is a no-op (`Ok(None)`) when a
    /// row with that key already exists — used so always-on recurring jobs
    /// self-heal across restarts. `max_attempts` falls back to the schema default.
    pub async fn enqueue(
        &self,
        kind: &str,
        payload: &JobPayload,
        run_at: DateTime<Utc>,
        max_attempts: Option<i32>,
        idempotency_key: Option<&str>,
    ) -> Result<Option<i64>, JobRepoError> {
        let id = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO background_job (kind, payload, run_at, max_attempts, idempotency_key)
            VALUES ($1, $2, $3, COALESCE($4, 5), $5)
            ON CONFLICT (idempotency_key) DO NOTHING
            RETURNING id
            "#,
        )
        .bind(kind)
        .bind(payload)
        .bind(run_at)
        .bind(max_attempts)
        .bind(idempotency_key)
        .fetch_optional(self.db.pool())
        .await
        .map_err(JobRepoError::Db)?;
        Ok(id)
    }

    /// Atomically claim up to `batch` due, unleased `pending` jobs for this
    /// worker, giving each a lease of `lease_ttl`. Returns the claimed rows.
    ///
    /// Claims across every `kind` — what production always wants (one queue,
    /// every registered handler). Tests that seed rows of their own kind
    /// alongside a concurrently-running suite want [`Self::claim_kinds`]
    /// instead, or they steal each other's rows.
    pub async fn claim(
        &self,
        batch: usize,
        worker_id: &str,
        lease_ttl: std::time::Duration,
    ) -> Result<Vec<ClaimedJob>, JobRepoError> {
        self.claim_inner(batch, worker_id, lease_ttl, None).await
    }

    /// Like [`Self::claim`], restricted to `kind = ANY(kinds)`.
    ///
    /// Test-only scoping: every job test seeds a `kind` unique to that test
    /// (e.g. a `test.<module>.<pid>` prefix) and claims through this method
    /// instead of [`Self::claim`], so concurrent tests — and the suite's own
    /// background worker, if one is running — can never claim each other's
    /// rows. Production code never needs this: a real worker wants every
    /// registered kind.
    pub async fn claim_kinds(
        &self,
        batch: usize,
        worker_id: &str,
        lease_ttl: std::time::Duration,
        kinds: &[&str],
    ) -> Result<Vec<ClaimedJob>, JobRepoError> {
        self.claim_inner(batch, worker_id, lease_ttl, Some(kinds))
            .await
    }

    async fn claim_inner(
        &self,
        batch: usize,
        worker_id: &str,
        lease_ttl: std::time::Duration,
        kinds: Option<&[&str]>,
    ) -> Result<Vec<ClaimedJob>, JobRepoError> {
        // Not a compile-time-checked `query_as!` (this crate builds without a
        // database; see the module-level note), so branching the SQL text on
        // whether a kind filter was asked for costs nothing extra.
        let kind_clause = if kinds.is_some() {
            "AND kind = ANY($4)"
        } else {
            ""
        };
        let sql = format!(
            r#"
            WITH candidate AS (
                SELECT id FROM background_job
                WHERE (state = 'pending' OR (state = 'running' AND lease_expires_at < now()))
                  AND run_at <= now()
                  {kind_clause}
                ORDER BY run_at, id
                FOR UPDATE SKIP LOCKED
                LIMIT $1
            )
            UPDATE background_job j
            SET state = 'running', claimed_by = $2,
                lease_expires_at = now() + ($3 * interval '1 second'),
                heartbeat_at = now(), started_at = COALESCE(started_at, now()),
                attempts = attempts + 1, updated_at = now()
            FROM candidate c
            WHERE j.id = c.id
            RETURNING j.id, j.kind, j.payload, j.attempts, j.max_attempts, j.schedule
            "#
        );
        let mut query = sqlx::query_as::<_, ClaimedJob>(&sql)
            .bind(batch as i64)
            .bind(worker_id)
            .bind(lease_ttl.as_secs() as i32);
        if let Some(kinds) = kinds {
            let owned: Vec<String> = kinds.iter().map(|k| (*k).to_string()).collect();
            query = query.bind(owned);
        }
        let rows = query
            .fetch_all(self.db.pool())
            .await
            .map_err(JobRepoError::Db)?;
        Ok(rows)
    }

    /// Refresh a running job's lease (called on a timer while a long handler runs).
    pub async fn heartbeat(
        &self,
        id: i64,
        worker_id: &str,
        lease_ttl: std::time::Duration,
    ) -> Result<(), JobRepoError> {
        sqlx::query(
            r#"
            UPDATE background_job
            SET heartbeat_at = now(),
                lease_expires_at = now() + ($2 * interval '1 second'),
                updated_at = now()
            WHERE id = $1 AND state = 'running' AND claimed_by = $3
            "#,
        )
        .bind(id)
        .bind(lease_ttl.as_secs() as i32)
        .bind(worker_id)
        .execute(self.db.pool())
        .await
        .map_err(JobRepoError::Db)?;
        Ok(())
    }

    /// Mark a job successful. `next_run_at = Some(t)` reschedules a *recurring*
    /// job (`state` back to `pending`, attempts cleared); `None` completes a
    /// one-shot job (`state = 'succeeded'`). `finished_at` records the last-success
    /// time. Scoped to `claimed_by = worker_id` so a zombie worker that wakes up
    /// after its lease was reclaimed cannot stomp on the new claim.
    pub async fn finish_success(
        &self,
        id: i64,
        worker_id: &str,
        next_run_at: Option<DateTime<Utc>>,
        finished_at: DateTime<Utc>,
    ) -> Result<(), JobRepoError> {
        if let Some(next) = next_run_at {
            sqlx::query(
                r#"
                UPDATE background_job
                SET state = 'pending', attempts = 0, run_at = $2,
                    claimed_by = NULL, lease_expires_at = NULL, heartbeat_at = NULL,
                    started_at = NULL, last_error = NULL, finished_at = $3, updated_at = now()
                WHERE id = $1 AND claimed_by = $4
                "#,
            )
            .bind(id)
            .bind(next)
            .bind(finished_at)
            .bind(worker_id)
            .execute(self.db.pool())
            .await
            .map_err(JobRepoError::Db)?;
        } else {
            sqlx::query(
                r#"
                UPDATE background_job
                SET state = 'succeeded', finished_at = $2,
                    claimed_by = NULL, lease_expires_at = NULL, heartbeat_at = NULL,
                    started_at = NULL, updated_at = now()
                WHERE id = $1 AND claimed_by = $3
                "#,
            )
            .bind(id)
            .bind(finished_at)
            .bind(worker_id)
            .execute(self.db.pool())
            .await
            .map_err(JobRepoError::Db)?;
        }
        Ok(())
    }

    /// Requeue a transient failure at `run_at`, recording `error`. Scoped to
    /// `claimed_by = worker_id` (see [`Self::finish_success`]).
    pub async fn retry(
        &self,
        id: i64,
        worker_id: &str,
        error: &str,
        run_at: DateTime<Utc>,
    ) -> Result<(), JobRepoError> {
        sqlx::query(
            r#"
            UPDATE background_job
            SET state = 'pending', run_at = $2, last_error = $3,
                claimed_by = NULL, lease_expires_at = NULL, heartbeat_at = NULL,
                started_at = NULL, updated_at = now()
            WHERE id = $1 AND claimed_by = $4
            "#,
        )
        .bind(id)
        .bind(run_at)
        .bind(error)
        .bind(worker_id)
        .execute(self.db.pool())
        .await
        .map_err(JobRepoError::Db)?;
        Ok(())
    }

    /// Dead-letter a job: mark it `failed` with `error` for inspection. The row
    /// is later removed by `jobs.gc`. Scoped to `claimed_by = worker_id` (see
    /// [`Self::finish_success`]).
    pub async fn fail(&self, id: i64, worker_id: &str, error: &str) -> Result<(), JobRepoError> {
        sqlx::query(
            r#"
            UPDATE background_job
            SET state = 'failed', finished_at = now(), last_error = $2,
                claimed_by = NULL, lease_expires_at = NULL, heartbeat_at = NULL, updated_at = now()
            WHERE id = $1 AND claimed_by = $3
            "#,
        )
        .bind(id)
        .bind(error)
        .bind(worker_id)
        .execute(self.db.pool())
        .await
        .map_err(JobRepoError::Db)?;
        Ok(())
    }

    /// Delete terminal (`succeeded`/`failed`) rows whose `finished_at` is before
    /// `cutoff`. Returns the number of rows removed.
    pub async fn gc(&self, cutoff: DateTime<Utc>) -> Result<u64, JobRepoError> {
        let res = sqlx::query(
            "DELETE FROM background_job WHERE state IN ('succeeded', 'failed') AND finished_at < $1",
        )
        .bind(cutoff)
        .execute(self.db.pool())
        .await
        .map_err(JobRepoError::Db)?;
        Ok(res.rows_affected())
    }
}
