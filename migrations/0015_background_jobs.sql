-- Background job queue (plans/m9-background-jobs.md). A durable one-shot +
-- recurring job table claimed by in-process workers. All timestamps are
-- TIMESTAMPTZ (UTC); there is no per-job timezone anywhere in the system.

CREATE TABLE background_job (
    id                BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    -- Discriminator + versioned JSON payload (each kind versions its own shape).
    kind              TEXT        NOT NULL,
    payload           JSONB       NOT NULL DEFAULT '{}'::jsonb,
    state             TEXT        NOT NULL DEFAULT 'pending'
                      CHECK (state IN ('pending', 'running', 'succeeded', 'failed')),
    attempts          INT         NOT NULL DEFAULT 0,
    max_attempts      INT         NOT NULL DEFAULT 5,
    -- Earliest allowed run (the "min scheduled date"). Claimable when run_at <= now().
    run_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- NULL = one-shot; {"every_seconds":N} or {"cron":"..."} = recurring (UTC).
    schedule          JSONB,
    -- Lease: only meaningful while state = 'running'.
    claimed_by        TEXT,
    lease_expires_at  TIMESTAMPTZ,
    heartbeat_at      TIMESTAMPTZ,
    -- Result bookkeeping.
    last_error        TEXT,
    started_at        TIMESTAMPTZ,
    finished_at       TIMESTAMPTZ,
    -- Enqueue-time idempotency (recurring rows use a stable key for self-healing bootstrap).
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
    WHERE state IN ('succeeded', 'failed');
