-- Location photos (M4 pulls the full moderation pipeline forward; M1 seeds
-- pre-approved images through the object-storage port).
--
-- A photo references a `parking_location` and an opaque object-storage key
-- (the bytes live behind the ObjectStorage port — local disk in dev, S3-style
-- later). `moderation_state` defaults to APPROVED so seeded/location images are
-- publicly visible now; M4 introduces the PENDING_REVIEW → APPROVED/REJECTED
-- flow and flips the default.

CREATE TABLE parking_photo (
    id               BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    location_id      BIGINT NOT NULL REFERENCES parking_location(id) ON DELETE CASCADE,
    storage_key      TEXT NOT NULL,                       -- opaque ObjectStorage key
    content_type     TEXT NOT NULL,                       -- e.g. 'image/jpeg'
    alt              TEXT,                                -- accessible description
    position         INTEGER NOT NULL DEFAULT 0,          -- ordering within a location
    moderation_state TEXT NOT NULL DEFAULT 'APPROVED'
        CHECK (moderation_state IN ('PENDING_REVIEW', 'APPROVED', 'REJECTED')),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    seed_key         TEXT                                 -- dev-only affordance (Ledger #1)
);

-- Fast "approved photos for a location, in order" lookups (search + details).
CREATE INDEX parking_photo_location_idx
    ON parking_photo (location_id, position, id);
