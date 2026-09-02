-- §43/§44/§47: reports, photo hide state, audit-viewer indexes.
-- See plans/m5-moderation.md §3.

CREATE TABLE report (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    reporter_id     BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    target_type     TEXT NOT NULL CHECK (target_type IN
                        ('parking', 'parking_photo', 'review', 'review_photo')),
    target_id       BIGINT NOT NULL,        -- row id in the target table (no FK: polymorphic)
    reason          TEXT NOT NULL,          -- domain code from REPORT_REASONS (§43)
    description     TEXT,                   -- optional, <= 1000 chars (§103)
    state           TEXT NOT NULL DEFAULT 'OPEN'
                    CHECK (state IN ('OPEN', 'UNDER_REVIEW', 'RESOLVED', 'DISMISSED')),
    claimed_by      BIGINT REFERENCES users(id) ON DELETE SET NULL,
    resolved_by     BIGINT REFERENCES users(id) ON DELETE SET NULL,
    resolution_note TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX report_state_idx     ON report (state, created_at);
CREATE INDEX report_target_idx    ON report (target_type, target_id);
CREATE INDEX report_reporter_idx  ON report (reporter_id, created_at DESC);

-- Photo moderation gains HIDDEN (§44 hide/restore). Drop + re-add the CHECK
-- (the auto-generated name is <table>_<column>_check).
ALTER TABLE parking_photo DROP CONSTRAINT parking_photo_moderation_state_check;
ALTER TABLE parking_photo ADD CONSTRAINT parking_photo_moderation_state_check
    CHECK (moderation_state IN ('PENDING_REVIEW', 'APPROVED', 'REJECTED', 'HIDDEN'));

-- Audit viewer (§47): filter by target and time.
CREATE INDEX audit_events_target_idx  ON audit_events (target_type, target_id);
CREATE INDEX audit_events_created_idx ON audit_events (created_at DESC);
