# M9 — PostgreSQL-backed background jobs — implementation plan

> **Status: planned.** A pure-PostgreSQL job queue + in-process workers, so the app can
> push long-running or deferred work (retention, media/export, later email + thumbnail
> processing) off the request path without adding a separate queue broker.
> Parent plan: `PLAN.md`.

## Goal

A durable **one-shot + recurring** background job system with **no new broker**: jobs live in a
`background_job` table; workers **(one per app instance, in-process)** atomically **claim** due jobs,
run them, record success/failure, **retry with exponential backoff + jitter**, **dead-letter** jobs
that exhaust their attempt budget, and delete terminal rows after **7 days**. Recurring jobs
self-schedule their next run. Adding new job kinds is a row + a handler — no schema change.

## Context: what already exists (build on, don't rebuild)

- All batch work today is **one-shot subcommands** in `crates/web/src/main.rs` (`retention`,
  `seed-admin`, `seed-policies`) that connect, run once, exit. There is **no in-process worker
  loop** — the web server is a long-lived axum process (`serve()`).
- **Sqlx runtime queries** (`query_as::<_, T>(...).bind(...)`, `FromRow`) are the norm since the
  compile-time macros were dropped; `cargo build` is DB-free. Repos wrap `Db { pool }`
  (`crates/infrastructure/src/db.rs`).
- **Application layer = use cases + ports** (`async_trait`), implemented by infra `Sqlx*Repository`.
  Config is env-driven structs (`crates/infrastructure/src/config.rs`).
- Existing near-identical job logic to reuse: **`RetentionJob`** (application use case, already a
  one-shot run) and the retention/export repositories. The media orphan sweep is a **no-op for S3**
  (no filesystem), so it won't be a first job.

## Decisions (confirmed)

| Question | Decision |
|---|---|
| Worker host | **In-process** — spawn a tokio task loop in `serve()`; env-guarded off for web-only instances |
| Scheduling | **One-shot + recurring** — a `run_at` "earliest allowed" timestamp plus a `schedule` JSONB for recurrence (`every_seconds` or a **UTC cron**). **All job times are UTC.** |
| Registry | **Generic** — `kind` + versioned `payload` JSONB + per-kind handler map (registry in infra) |
| Retry | **Backoff + dead-letter** — exponential backoff + jitter, `max_attempts` cap (default 5), terminal `failed` row kept |
| Retention | **GC job** — a recurring `jobs.gc` deletes terminal rows older than 7 days |

## Migration — `migrations/0014_background_jobs.sql`

```sql
CREATE TABLE background_job (
    id                BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind              TEXT        NOT NULL,                 -- 'retention', 'jobs.gc', 'media.sweep', ...
    payload           JSONB       NOT NULL DEFAULT '{}'::jsonb,   -- versioned per kind
    state             TEXT        NOT NULL DEFAULT 'pending'
                      CHECK (state IN ('pending','running','succeeded','failed')),
    attempts          INT         NOT NULL DEFAULT 0,
    max_attempts      INT         NOT NULL DEFAULT 5,
    -- Scheduling: earliest allowed run (the "min scheduled date") + optional recurrence.
    run_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    schedule          JSONB,  -- NULL=one-shot; {"every_seconds":N} or {"cron":"..."} = recurring (UTC)
    -- Lease (only meaningful while state='running').
    claimed_by        TEXT,
    lease_expires_at  TIMESTAMPTZ,
    heartbeat_at      TIMESTAMPTZ,
    -- Result bookkeeping.
    last_error        TEXT,
    started_at        TIMESTAMPTZ,
    finished_at       TIMESTAMPTZ,
    -- Enqueue-time idempotency (recurring rows use a stable key so bootstrap is self-healing).
    idempotency_key   TEXT UNIQUE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Due, unleased pending jobs (drives the claim query).
CREATE INDEX background_job_due
    ON background_job (run_at, id)
    WHERE state = 'pending';

-- GC sweep.
CREATE INDEX background_job_terminal
    ON background_job (finished_at)
    WHERE state IN ('succeeded','failed');
```

Notes on the schema:

- `run_at` is the **minimum** date the job is allowed to start (your "scheduled_date"). A job is
  claimable when `run_at <= now()`.
- **All job timestamps are `TIMESTAMPTZ` = UTC.** `run_at`, `lease_expires_at`, `heartbeat_at`,
  `started_at`, `finished_at` are all stored as UTC instants; the worker clock is `Utc`. There is no
  per-job timezone — nowhere in the system is a wall-clock or a local timezone interpreted for
  scheduling.
- `schedule = NULL` ⇒ one-shot (runs once, goes terminal). `schedule = {"every_seconds": 86400}` ⇒
  recurring on a fixed interval. `schedule = {"cron": "0 3 * * *"}` ⇒ recurring on a **cron
  expression evaluated in UTC** (a Unix 5-field `min hour dom month dow`, normalized to the
  seconds-based form — minute resolution). On success the worker recomputes `run_at` from the
  schedule and resets to `pending`.
- **Recurring rows never go terminal** (they stay `pending`), so the GC never deletes them — but a
  recurring job that exhausts its attempts does go `failed` (terminal) and stops, which is correct
  (it dead-lettered).
- Enqueue-time idempotency via `idempotency_key` makes bootstrap of always-on recurring jobs
  self-healing (`INSERT … ON CONFLICT DO NOTHING`).

## Application layer — `crates/application/src/jobs.rs`

New module (exports from `lib.rs`), pure ports/vals, no SQL. Depends only on the domain.

```rust
/// A typed handler for one job `kind`. Infra builds concrete handlers that capture
/// the ports they need (retention repo, audit, clock …).
#[async_trait]
pub trait JobHandler: Send + Sync {
    fn kind(&self) -> &'static str;
    /// Runs the job. `Ok(())` = success; `Err(String)` = failure (may retry).
    /// Implementations MUST tolerate at-least-once execution (a crash can re-claim).
    async fn run(
        &self,
        engine: &JobEngine,
        payload: &JobPayload,
    ) -> Result<JobOutcome, JobError>;
}

/// Versioned payload envelope: kinds version their own payload shape themselves.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobPayload(serde_json::Value);   // raw JSONB; handlers deserialize to a typed struct

/// Result of a single run.
#[derive(Debug)]
pub enum JobOutcome {
    Success,
    /// One-shot + recurring both return Success; rescheduling is worker-driven via `schedule`.
}

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("job failed: {0}")]
    Failed(String),
    #[error("job failed permanently (no retry)")]
    Permanent(String),
}
```

- `JobEngine` is a small handle a handler can use to **enqueue a follow-up job** (e.g. retention
  enqueues a media sweep) — an `EnqueueJob` port. Minimal for M9.
- `JobError::Failed` ⇒ backoff + retry (if attempts remain); `JobError::Permanent` ⇒ dead-letter
  immediately.
- The **retry-vs-dead-letter decision** stays in the infra worker (needs the row's `attempts`/`max_attempts`).

## Infrastructure layer — `crates/infrastructure/src/job/`

New module tree; repos wrap `Db` like every other `Sqlx*Repository`.

### `job/mod.rs` — `SqlxJobRepository`

Implements the queue operations (port-ish; infra-local is fine for M9, we can lift a port later):

- `enqueue(kind, payload, run_at, max_attempts, idempotency_key) -> Result<Option<i64>>` —
  `INSERT … ON CONFLICT (idempotency_key) DO NOTHING`; returns the id, or `None` if the key already
  existed.
- `claim(batch, worker_id, lease_ttl) -> Vec<ClaimedJob>` — the heart of the queue:

  ```sql
  WITH candidate AS (
      SELECT id
      FROM background_job
      WHERE state = 'pending'
        AND run_at <= now()
        AND (lease_expires_at IS NULL OR lease_expires_at < now())
      ORDER BY run_at, id
      FOR UPDATE SKIP LOCKED
      LIMIT $1
  )
  UPDATE background_job j
  SET state = 'running', claimed_by = $2,
      lease_expires_at = now() + $3::interval, heartbeat_at = now(),
      started_at = COALESCE(started_at, now()), attempts = attempts + 1
  FROM candidate c
  WHERE j.id = c.id
  RETURNING j.id, j.kind, j.payload, j.attempts, j.max_attempts;
  ```

  `FOR UPDATE SKIP LOCKED` lets multiple in-process workers (or instances) claim different rows
  without blocking. Attempts are incremented **at claim** so a claim-that-crashes still counts and
  cannot loop forever.
- `heartbeat(id, worker_id, lease_ttl)` — extend `lease_expires_at`, set `heartbeat_at`.
- `finish_success(id, schedule, now)` — one-shot: `state='succeeded', finished_at=now()`, clear
  lease; recurring: `state='pending', attempts=0`, clear lease/`last_error`, set `finished_at=now()`
  (last-ran; not terminal so GC ignores it), and set `run_at` to the **next occurrence**: for
  `every_seconds` that is `now + Ns`; for `cron` it is the next tick after `now` in **UTC**. Missed
  occurrences while the worker was down are **skipped** (no catch-up burst) — the job resumes at the
  next future tick.
- `retry(id, error, backoff)` — if `attempts < max_attempts`: `state='pending',
  run_at = now() + backoff, last_error = error`, clear lease; else `fail()`. (Attempts already
  incremented at claim.)
- `fail(id, error)` — `state='failed', finished_at=now(), last_error = error`, clear lease. (Dead-letter.)
- `gc(cutoff)` — `DELETE WHERE state IN ('succeeded','failed') AND finished_at < $1`.

### `job/worker.rs` — the in-process loop

```rust
pub struct Worker { repo, registry, id, poll_interval, batch_size, lease_ttl, backoff_base }

pub async fn run(&self) {
    // Bootstrap always-on recurring jobs (self-healing): for each registered recurring
    // kind, `enqueue(kind, Some(payload), run_at=now, None, Some(idem_key))` — a no-op if present.
    loop {
        match self.repo.claim(batch, id, lease_ttl).await {
            Ok(jobs) if jobs.is_empty() => { sleep(poll_interval); continue; }
            Ok(jobs) => for job in jobs { self.heartbeat_and_run(job).await; }
            Err(e) => { warn!(...); sleep(poll_interval); }
        }
    }
}

async fn heartbeat_and_run(&self, job) {
    // While the handler runs, a helper task refreshes the lease every lease_ttl/3 so a
    // long job is not reclaimed mid-run.
    let handler = self.registry.get(&job.kind);  // unknown kind → permanent fail
    match handler.run(&engine, &job.payload).await {
        Ok(_)  => repo.finish_success(job.id, job.schedule, now).await,
        Err(JobError::Failed(e))  => repo.retry(job.id, &e, backoff(job.attempts)).await,
        Err(JobError::Permanent(e)) => repo.fail(job.id, &e).await,
    }
    // tracing span per run: kind, id, attempt, outcome, elapsed.
}
```

- **At-least-once** semantics: a crash mid-run leaves the row leasable; after `lease_ttl` it is
  re-claimed. Handlers must be idempotent (retention purge already is).
- Defaults: `poll_interval` 5 s, `batch_size` 4, `lease_ttl` 10 min (heartbeat every ~3 min),
  `backoff_base` 2 s (exponential × jitter), `max_attempts` 5.

### `job/registry.rs` + `job/handlers.rs`

- `JobRegistry { map: HashMap<String, Box<dyn JobHandler>>, recurring: Vec<RecurringKind> }` with
  `get(kind)`, `recurring_kinds()`.
- `job_registry(config, db, storage, audit, clock) -> JobRegistry` builds the built-in handlers
  (infra, like `storage_from_env`).
- **Cron dependency:** `cron` (or `croner`) in `crates/infrastructure/Cargo.toml`, used to parse a
  five-field expression and compute the next tick **after `now` in UTC** (`Schedule::upcoming(Utc)`
  or the croner equivalent). The next tick is always computed from `Utc::now()`, never a local tz or a
  per-job tz.
- **First handlers:**
  - `RetentionJobHandler` — wraps the existing `RetentionJob` application use case (repos + audit +
    clock + config). Short-circuits to `Success` when both anonymize/purge thresholds are `0` so the
    always-on daily run is a cheap no-op. Recurring `schedule = {"every_seconds": 86400}`,
    `idempotency_key = "recurring:retention"`.
  - `JobsGcHandler` — calls `repo.gc(now - JOBS_HISTORY_RETENTION_DAYS)`. Recurring daily,
    `idempotency_key = "recurring:jobs.gc"`. It never deletes its own row (it stays `pending`), so
    the queue stays clean forever.
  - `MediaSweepHandler` — **stubbed** (returns `Success` with a `debug!` note: S3 has no local FS to
    walk; bucket-list cleanup is a follow-up). Registered but disabled-by-config so a future
    implementation slots in without a schema change.

### Config additions — `config.rs`

```rust
pub jobs: JobConfig,
```

```rust
pub struct JobConfig {
    pub enabled:             bool,       // JOBS_ENABLED        (default true)
    pub poll_interval:       Duration,   // JOBS_POLL_INTERVAL_MS (default 5000)
    pub batch_size:          usize,      // JOBS_BATCH_SIZE       (default 4)
    pub lease_ttl:           Duration,   // JOBS_LEASE_TTL_MS      (default 600000)
    pub max_attempts:        usize,      // JOBS_MAX_ATTEMPTS      (default 5)
    pub backoff_base_ms:     u64,        // JOBS_BACKOFF_BASE_MS    (default 2000)
    pub history_retention_days: u32,     // JOBS_HISTORY_RETENTION_DAYS (default 7)
}
```

## Web wiring — `crates/web/src/main.rs` `serve()`

After `db.migrate()` and before `axum::serve`:

```rust
let registry = job_registry(config.clone(), db.clone(), storage_from_env(),
                            audit, clock);
if config.jobs.enabled {
    let worker = Worker::new(db.clone(), registry, config.jobs);
    tokio::spawn(async move { worker.run().await });     // in-process
    tracing::info!(jobs = "enabled", "background worker started");
} else {
    tracing::info!(jobs = "disabled", "background worker not started");
}
```

- `JOBS_ENABLED=false` gives a pure web instance (no worker task), for web-only replicas or when
  jobs are run by a separate instance.

## Enqueue API (request path)

A small `EnqueueJob` handle (impl of an `EnqueueJob` port) is threaded into `AppState` so request
handlers can fire-and-forget deferred work that must not block the HTTP response — e.g. a future
`privacy.run_export`, email delivery, or image re-processing. It is just `repo.enqueue(kind, payload,
run_at=now, max_attempts, None)` with the row surviving across instances. **Optional for M9** — the
worker + retention + GC ship first; request-path enqueue wires in when the first caller needs it.

## Testing strategy

- **Unit (no DB):** backoff computation (exponential × jitter, capped), retry-vs-dead-letter
  threshold logic, recurring next-`run_at` derivation for **`every_seconds` and `cron`** (next tick
  after `now`, UTC — e.g. `"0 3 * * *"` at `02:00 UTC` yields `03:00 UTC` same day), and GC deletion
  predicate. Cron parse of an invalid expression is a config-time error (register the recurring job
  at startup and bail loudly rather than silently never firing).
- **Gated integration (needs the DB, like other infra tests):** `job_test.rs` using the db_test
  harness —
  - claim only picks due, unleased, `pending` rows; skips leased; `SKIP LOCKED` lets two workers
    claim disjoint batches (spawn 2 concurrent claims, assert no overlap).
  - success → one-shot goes `succeeded`; recurring resets to `pending` with the next `run_at` and
    `attempts=0`.
  - failure → backoff retry updates `run_at`/`last_error`; after `max_attempts` → `failed` (dead-letter).
  - lease expiry → a running row past `lease_ttl` is re-claimable.
  - `gc` deletes only `succeeded`/`failed` older than the cutoff; never pending/recurring.
  - bootstrap idempotency: `enqueue` with a recurring idempotency_key twice → one row.
  - `RetentionJobHandler` no-ops when thresholds are `0`.

## Security, observability & ops

- **At-least-once** is the contract: handlers stay idempotent. Document per kind.
- Each run emits a `tracing::info_span!` (`kind`, `id`, `attempt`) with outcome + elapsed; failures
  log `last_error` and the job payload id (never the full payload, which may contain PII). Terminal
  failures should also be mirrored to the existing **audit log** if the kind is a regulatory concern
  (retention already audits its own steps).
- **GC deletes `failed` rows after 7 days**, so deep investigation of a dead-lettered job must use
  the audit/error log within the window; the queue is an execution surface, not a permanent audit trail.
- Lease + heartbeat prevent a wedged worker from shredding the budget; a single instance runs
  one worker (multi-instance is safe via `SKIP LOCKED`).

## Rollout

1. Add migration `0014_background_jobs.sql`.
2. Add `application::jobs` module (handler/outcome/error/engine) + exports.
3. Add `infrastructure::job` (repo, worker, registry, handlers) + `storage_from_env`-style builder.
4. Add `JobConfig` + env docs (`.env.example`, `docs/deployment.md`).
5. Wire the worker into `serve()`; keep `retention` subcommand as a manual escape hatch.
6. Tests (above) + `cargo build --workspace` DB-free; full suite green.
7. Docs: `PENDING_FOR_PRODUCTION.md` (add the job system, mark "async export worker" etc. as now
   possible), `PLAN.md`/`README.md` env table.

## Out of scope / follow-ups

- **Timezone-aware cron (per-job IANA zones / DST)** — intentionally **not** supported. All cron and
  scheduled times are **UTC only** (documented invariant). If a future feature truly needs "run at
  09:00 in São Paulo," that must be a deliberate extension with an explicit per-job tz field — out of
  scope now.
- **HTTP admin surface** (list/requeue/fail a job from the UI) and job metrics/GAUGE gauges.
- **Direct S3 presigned-URL serving for the media device** is unaffected; the media-sweep job stays stubbed.
- **Worker scaling/concurrency tuning** (run jobs in parallel up to a `jobs_concurrency` cap instead
  of sequential) once real throughput is needed.
- Request-path **enqueue port** wiring + first caller (async export assembly, email delivery).

---

**Acceptance (definition of done):** a `background_job` table that pending jobs claim/run/retry so
the request path never blocks; retention and a `jobs.gc` cleanup run as recurring in-process jobs
(auto-bootstrapped, no manual seed); terminal rows are deleted after 7 days; `cargo build --workspace`
is DB-free and the full test suite (incl. gated job tests against the DB) is green.
