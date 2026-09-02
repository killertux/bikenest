-- M1: parking locations, security features, opening hours.
-- See plans/m1-search-map.md §3 for the design rationale.

-- Security feature catalog (§28). Extensible: add rows, no schema change.
CREATE TABLE security_feature (
    code  TEXT PRIMARY KEY,
    label TEXT NOT NULL
);

INSERT INTO security_feature (code, label) VALUES
    ('dedicated_locking_point', 'Dedicated locking point'),
    ('indoor',                  'Indoor'),
    ('cctv',                    'CCTV'),
    ('staffed',                 'Staffed'),
    ('security_guard',          'Security guard'),
    ('controlled_access',       'Controlled access'),
    ('well_lit',                'Well lit'),
    ('restricted_access',       'Restricted access');

CREATE TABLE parking_location (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name          TEXT NOT NULL,
    address       TEXT NOT NULL,
    description   TEXT,
    parking_type  TEXT NOT NULL,             -- domain-validated enum (§26)
    cost_kind     TEXT NOT NULL,             -- 'free' | 'paid' | 'unknown' (§27)
    price_cents   BIGINT,
    price_currency CHAR(3),
    price_unit    TEXT,                      -- 'hour' | 'day' | 'month' | 'entry'
    location      geography(Point, 4326) NOT NULL,
    lat           double precision GENERATED ALWAYS AS (ST_Y(location::geometry)) STORED,
    lon           double precision GENERATED ALWAYS AS (ST_X(location::geometry)) STORED,
    timezone      TEXT NOT NULL,             -- IANA identifier (§29)
    hours_unknown BOOLEAN NOT NULL DEFAULT false,  -- true = hours unknown (§29)
    rating_avg    NUMERIC(3,2),              -- denormalized aggregates; maintained by
    rating_count  INTEGER NOT NULL DEFAULT 0, -- review use cases from M3; seeder fills in M1
    moderation_state TEXT NOT NULL DEFAULT 'ACTIVE',  -- §25 lifecycle
    created_at                 TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                 TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_meaningful_update_at  TIMESTAMPTZ,
    last_verified_at           TIMESTAMPTZ,
    seed_key        TEXT                     -- dev-only marker for mock data (Ledger #13)
);

CREATE INDEX parking_location_location_gist ON parking_location USING GIST (location);
CREATE INDEX parking_location_state_idx ON parking_location (moderation_state);

-- Tri-state security attributes per location (§28): 0 unknown, 1 yes, 2 no.
CREATE TABLE parking_security (
    location_id  BIGINT NOT NULL REFERENCES parking_location(id) ON DELETE CASCADE,
    feature_code TEXT NOT NULL REFERENCES security_feature(code),
    state        SMALLINT NOT NULL CHECK (state IN (0, 1, 2)),
    PRIMARY KEY (location_id, feature_code)
);

-- Weekly wall-clock opening hours in the location's own timezone (§29).
-- Multiple rows per day allowed; all_day = 24h; day_of_week ISO 1=Mon..7=Sun.
CREATE TABLE opening_hours (
    location_id  BIGINT NOT NULL REFERENCES parking_location(id) ON DELETE CASCADE,
    day_of_week  SMALLINT NOT NULL CHECK (day_of_week BETWEEN 1 AND 7),
    opens_at     TIME NOT NULL,
    closes_at    TIME NOT NULL,
    all_day      BOOLEAN NOT NULL DEFAULT false,
    PRIMARY KEY (location_id, day_of_week, opens_at, closes_at)
);
