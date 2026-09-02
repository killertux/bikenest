-- M3: contribution schema — optimistic concurrency + creator capture + field-level history + proposals.
-- See plans/m3-community.md §3.

-- §100 optimistic concurrency + §35 creator capture.
ALTER TABLE parking_location
    ADD COLUMN version     BIGINT NOT NULL DEFAULT 1,
    ADD COLUMN creator_id  BIGINT REFERENCES users(id) ON DELETE SET NULL;
-- creator_id is stored for internal attribution only; never rendered publicly (§35/§46).

-- Immutable field-level history of APPLIED changes (§107). One row per change;
-- version 1 = creation. `snapshot` is the AFTER-state of the tracked fields.
CREATE TABLE parking_revision (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    location_id  BIGINT NOT NULL REFERENCES parking_location(id) ON DELETE CASCADE,
    version      BIGINT NOT NULL,                 -- parking_location.version AFTER this change
    editor_id    BIGINT REFERENCES users(id) ON DELETE SET NULL,  -- NULL = system/seed
    change_kind  TEXT NOT NULL,                   -- 'create' | 'edit' | 'moderation' | 'verification'
    summary      TEXT,                            -- short human description for C5
    snapshot     JSONB NOT NULL,                  -- {name,address,type,cost,point,tz,hours,security,moderation_state}
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (location_id, version)
);
CREATE INDEX parking_revision_location_idx ON parking_revision (location_id, version DESC);
CREATE INDEX parking_revision_editor_idx   ON parking_revision (editor_id, created_at DESC);

-- Gated sensitive changes (§37/§107): location move, existence/removal.
-- Created in M3 (PENDING); approved/rejected/modified in M5.
CREATE TABLE parking_proposal (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    location_id  BIGINT NOT NULL REFERENCES parking_location(id) ON DELETE CASCADE,
    proposer_id  BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    base_version BIGINT NOT NULL,                 -- parking_location.version at proposal time (§100)
    kind         TEXT NOT NULL CHECK (kind IN ('move_location', 'change_existence')),
    proposed     JSONB NOT NULL,                  -- move_location: {point, timezone, reason}
                                                  -- change_existence: {existence, reason}
    status       TEXT NOT NULL DEFAULT 'PENDING'
                 CHECK (status IN ('PENDING', 'APPROVED', 'REJECTED', 'SUPERSEDED')),
    resolved_by  BIGINT REFERENCES users(id) ON DELETE SET NULL,
    resolved_at  TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX parking_proposal_location_idx ON parking_proposal (location_id, status);
CREATE INDEX parking_proposal_status_idx   ON parking_proposal (status, created_at);
