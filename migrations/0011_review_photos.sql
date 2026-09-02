-- §38 review-photo attach (deferred from M4). Same storage/EXIF/thumbnail and
-- moderation contract as parking_photo (§30/§80): uploads are processed
-- derivatives only, held PENDING_REVIEW, APPROVED-only visible, HIDDEN
-- restorable.

CREATE TABLE review_photo (
    id               BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    review_id        BIGINT NOT NULL REFERENCES review(id) ON DELETE CASCADE,
    uploader_id      BIGINT REFERENCES users(id) ON DELETE SET NULL,
    storage_key      TEXT NOT NULL,
    thumbnail_key    TEXT,
    width            INTEGER,
    height           INTEGER,
    processed_at     TIMESTAMPTZ,
    moderation_state TEXT NOT NULL DEFAULT 'PENDING_REVIEW'
        CHECK (moderation_state IN ('PENDING_REVIEW', 'APPROVED', 'REJECTED', 'HIDDEN')),
    rejection_reason TEXT,
    reviewed_by      BIGINT REFERENCES users(id) ON DELETE SET NULL,
    reviewed_at      TIMESTAMPTZ,
    position         INTEGER NOT NULL DEFAULT 0,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX review_photo_pending_idx ON review_photo (moderation_state, created_at)
    WHERE moderation_state = 'PENDING_REVIEW';
CREATE INDEX review_photo_review_idx ON review_photo (review_id, position, id)
    WHERE moderation_state = 'APPROVED';
CREATE INDEX review_photo_uploader_idx ON review_photo (uploader_id, created_at DESC);
