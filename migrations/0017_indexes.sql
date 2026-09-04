-- Data-access efficiency: FK lookup indexes, read-path indexes for the
-- details/moderation queues, a few narrowing CHECK constraints, and two
-- superseded-index drops.
--
-- NOTE for production: this migration runs inside sqlx's migration
-- transaction, so every `CREATE INDEX` below takes a normal (non-concurrent)
-- lock for the duration of the build. That is fine against the seeded dev
-- database, but on a live database with real table sizes these indexes
-- should be built with `CREATE INDEX CONCURRENTLY` run out of band (outside
-- the migration transaction, one statement per connection, retried if it
-- fails partway) so writers are not blocked while the index builds.

-- ---------------------------------------------------------------------------
-- FK lookup indexes: columns that back a foreign key but had no index, so an
-- `ON DELETE SET NULL` (or an admin lookup "what did this user touch") forces
-- a sequential scan. All are partial on `IS NOT NULL` where the column is
-- usually null, to keep the index small.
-- ---------------------------------------------------------------------------

CREATE INDEX parking_location_creator_idx
    ON parking_location (creator_id)
    WHERE creator_id IS NOT NULL;

CREATE INDEX parking_proposal_proposer_idx
    ON parking_proposal (proposer_id, created_at DESC)
    WHERE proposer_id IS NOT NULL;

CREATE INDEX parking_proposal_resolved_by_idx
    ON parking_proposal (resolved_by)
    WHERE resolved_by IS NOT NULL;

CREATE INDEX report_claimed_by_idx
    ON report (claimed_by)
    WHERE claimed_by IS NOT NULL;

CREATE INDEX report_resolved_by_idx
    ON report (resolved_by)
    WHERE resolved_by IS NOT NULL;

CREATE INDEX parking_photo_reviewed_by_idx
    ON parking_photo (reviewed_by)
    WHERE reviewed_by IS NOT NULL;

CREATE INDEX review_photo_reviewed_by_idx
    ON review_photo (reviewed_by)
    WHERE reviewed_by IS NOT NULL;

CREATE INDEX user_roles_granted_by_idx
    ON user_roles (granted_by)
    WHERE granted_by IS NOT NULL;

CREATE INDEX privacy_request_fulfilled_by_idx
    ON privacy_request (fulfilled_by)
    WHERE fulfilled_by IS NOT NULL;

-- ---------------------------------------------------------------------------
-- parking_photo: the P3 gallery/card read is always `location_id + APPROVED`,
-- ordered `position, id`. `parking_photo_location_idx` (0003) has no state
-- predicate and stays: `PhotoRepository::max_position` computes
-- `MAX(position)` across *all* moderation states for a location (it must see
-- pending/rejected/hidden rows too, to keep new uploads from colliding on
-- position), so it still needs the unfiltered index.
-- ---------------------------------------------------------------------------

CREATE INDEX parking_photo_approved_idx
    ON parking_photo (location_id, position, id)
    WHERE moderation_state = 'APPROVED';

-- ---------------------------------------------------------------------------
-- verification: the community-details aggregate reads by (location, kind)
-- for the attribute-summary and parked-here-count queries, and additionally
-- narrows to `kind = 'existence'` for the latest-signal-per-user query.
-- `verification_location_idx` (0008, `location_id, created_at DESC`) is kept:
-- it's still the right shape for a plain "verification timeline" read.
-- ---------------------------------------------------------------------------

CREATE INDEX verification_location_kind_idx
    ON verification (location_id, kind, created_at DESC);

CREATE INDEX verification_existence_user_idx
    ON verification (location_id, user_id, created_at DESC)
    WHERE kind = 'existence';

-- ---------------------------------------------------------------------------
-- review: the details page's bounded, keyset-paginated `list_active` reads
-- `location_id + ACTIVE`, ordered `created_at DESC, id DESC`.
-- `review_location_idx` (0008) is kept for the unfiltered timeline shape.
-- ---------------------------------------------------------------------------

CREATE INDEX review_active_location_idx
    ON review (location_id, created_at DESC, id DESC)
    WHERE moderation_state = 'ACTIVE';

-- ---------------------------------------------------------------------------
-- audit_events: the admin viewer's keyset cursor is always `id DESC`, whether
-- or not it's narrowed by actor or time range. Add `id` as the trailing sort
-- column so a filtered page doesn't need a separate sort step.
-- ---------------------------------------------------------------------------

CREATE INDEX audit_events_actor_id_idx
    ON audit_events (actor_user_id, id DESC);

CREATE INDEX audit_events_created_id_idx
    ON audit_events (created_at DESC, id DESC);

-- ---------------------------------------------------------------------------
-- Misc read paths.
-- ---------------------------------------------------------------------------

-- Retention's `purge_deleted_accounts` (M6) sweeps by `deleted_at`, but only
-- rows already flipped to DELETED are candidates.
CREATE INDEX users_deleted_at_idx
    ON users (deleted_at)
    WHERE account_state = 'DELETED';

-- Session idle-timeout sweep (`purge_expired_sessions` compares `last_seen_at`).
CREATE INDEX sessions_last_seen_idx
    ON sessions (last_seen_at);

-- The job worker's lease-reclaim branch: running jobs whose lease has
-- expired (WP4). Partial on `state = 'running'` — pending/terminal jobs never
-- match this branch.
CREATE INDEX background_job_lease_idx
    ON background_job (lease_expires_at)
    WHERE state = 'running';

-- ---------------------------------------------------------------------------
-- CHECK constraints. Verified against the seeded dev database first (all
-- counts were 0 — no seed-data fix was required):
--   select count(*) from parking_location where cost_kind not in ('free','paid','unknown');            -- 0
--   select count(*) from parking_location where not (cost_kind = 'paid'
--       or (price_cents is null and price_currency is null and price_unit is null));                   -- 0
--   select count(*) from parking_location where not (price_cents is null or price_cents >= 0);         -- 0
--   select count(*) from parking_location where moderation_state not in
--       ('ACTIVE','PENDING_REVIEW','FLAGGED','INVALID','REMOVED');                                     -- 0
-- `review`, `parking_photo` and `review_photo` already carry a moderation_state
-- CHECK (0008/0010/0011); `parking_location` never did, so it gets one here.
-- ---------------------------------------------------------------------------

ALTER TABLE parking_location
    ADD CONSTRAINT parking_location_cost_kind_check
    CHECK (cost_kind IN ('free', 'paid', 'unknown'));

ALTER TABLE parking_location
    ADD CONSTRAINT parking_location_price_shape_check
    CHECK (cost_kind = 'paid' OR (price_cents IS NULL AND price_currency IS NULL AND price_unit IS NULL));

ALTER TABLE parking_location
    ADD CONSTRAINT parking_location_price_cents_check
    CHECK (price_cents IS NULL OR price_cents >= 0);

ALTER TABLE parking_location
    ADD CONSTRAINT parking_location_moderation_state_check
    CHECK (moderation_state IN ('ACTIVE', 'PENDING_REVIEW', 'FLAGGED', 'INVALID', 'REMOVED'));

-- ---------------------------------------------------------------------------
-- Superseded indexes.
-- ---------------------------------------------------------------------------

-- Covered by the UNIQUE (location_id, version) index (0007): a btree on
-- (location_id, version) serves `WHERE location_id = $1 ORDER BY version DESC`
-- just as well scanned backward.
DROP INDEX parking_revision_location_idx;

-- Replaced below by a partial index scoped to the moderation queue's actual
-- read (locations needing attention), instead of every location keyed by state.
DROP INDEX parking_location_state_idx;

CREATE INDEX parking_location_needs_review_idx
    ON parking_location (moderation_state, updated_at)
    WHERE moderation_state <> 'ACTIVE';
