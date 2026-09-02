-- §30/§116.2: real photo pipeline. M1 seeded pre-APPROVED originals; M4 adds
-- the upload→moderate columns and flips the default so new uploads are held
-- for review. `storage_key` is now the *full processed derivative*; the raw
-- upload is never stored (§80).

ALTER TABLE parking_photo
    ADD COLUMN uploader_id      BIGINT REFERENCES users(id) ON DELETE SET NULL,
    ADD COLUMN thumbnail_key    TEXT,                 -- processed thumbnail derivative
    ADD COLUMN width            INTEGER,              -- derivative pixel dimensions
    ADD COLUMN height           INTEGER,
    ADD COLUMN processed_at     TIMESTAMPTZ,          -- set when derivatives are stored
    ADD COLUMN rejection_reason TEXT,                 -- set by moderator on reject
    ADD COLUMN reviewed_by      BIGINT REFERENCES users(id) ON DELETE SET NULL,
    ADD COLUMN reviewed_at      TIMESTAMPTZ,
    ALTER COLUMN moderation_state SET DEFAULT 'PENDING_REVIEW';

-- Photo moderation queue (M2 screen), oldest first.
CREATE INDEX parking_photo_pending_idx
    ON parking_photo (moderation_state, created_at)
    WHERE moderation_state = 'PENDING_REVIEW';

-- Internal attribution / contributor history joins (never rendered publicly, §80).
CREATE INDEX parking_photo_uploader_idx
    ON parking_photo (uploader_id, created_at DESC);
