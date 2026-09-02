-- 0001_init: PostGIS baseline + minimal users table (M0).
-- Full auth schema arrives in M2 (authentication_identities, sessions, tokens).

CREATE EXTENSION IF NOT EXISTS postgis;

CREATE TABLE users (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    email         TEXT NOT NULL,
    display_name  TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Case-insensitive uniqueness; lookups use lower(email).
CREATE UNIQUE INDEX idx_users_email ON users (lower(email));
