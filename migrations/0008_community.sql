-- M3: community schema — reviews, verification signals and favorites.
-- See plans/m3-community.md §3.

-- Five-star reviews (§38). One row per user per location; edits update in place
-- and append to review_revision (history preserved).
CREATE TABLE review (
    id                BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    location_id       BIGINT NOT NULL REFERENCES parking_location(id) ON DELETE CASCADE,
    author_id         BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    rating            SMALLINT NOT NULL CHECK (rating BETWEEN 1 AND 5),
    body              TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 2000),
    moderation_state  TEXT NOT NULL DEFAULT 'ACTIVE'
                      CHECK (moderation_state IN ('ACTIVE', 'HIDDEN')),  -- HIDDEN set in M5
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (location_id, author_id)
);
CREATE INDEX review_location_idx ON review (location_id, created_at DESC);
CREATE INDEX review_author_idx   ON review (author_id, created_at DESC);

-- Review edit history (§38): one row per published version (initial + each edit).
CREATE TABLE review_revision (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    review_id  BIGINT NOT NULL REFERENCES review(id) ON DELETE CASCADE,
    rating     SMALLINT NOT NULL,
    body       TEXT NOT NULL,
    edited_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX review_revision_review_idx ON review_revision (review_id, id);

-- Verification signals (§39/§41). Multiple over time; aggregation uses the
-- latest per user. `expires_at` is set only for parked_here (now + 90 days).
CREATE TABLE verification (
    id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    location_id    BIGINT NOT NULL REFERENCES parking_location(id) ON DELETE CASCADE,
    user_id        BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind           TEXT NOT NULL CHECK (kind IN ('existence', 'attribute', 'parked_here')),
    result         TEXT NOT NULL,   -- existence: 'still_exists'|'no_longer_exists'|'info_changed'
                                    -- attribute: 'correct'|'incorrect'
                                    -- parked_here: 'parked_here'
    attribute_code TEXT,            -- for kind='attribute' (§39 per-attribute): name/address/type/cost/hours/security/location
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at     TIMESTAMPTZ      -- parked_here only
);
CREATE INDEX verification_location_idx  ON verification (location_id, created_at DESC);
CREATE INDEX verification_user_idx      ON verification (user_id, created_at DESC);
CREATE INDEX verification_parked_expiry ON verification (expires_at) WHERE kind = 'parked_here';

-- Favorites (§42): private, per user.
CREATE TABLE favorite (
    user_id     BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    location_id BIGINT NOT NULL REFERENCES parking_location(id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, location_id)
);
CREATE INDEX favorite_location_idx ON favorite (location_id);
