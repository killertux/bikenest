-- §70/§71: versioned legal pages. Content is seeded from policies/*.md via
-- `seed-policies`; text is PLACEHOLDER legal content requiring review (§71).
CREATE TABLE policy_version (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind          TEXT NOT NULL CHECK (kind IN ('privacy', 'terms', 'cookies')),
    version       TEXT NOT NULL,              -- e.g. '2025-01-01.1'
    effective_at  TIMESTAMPTZ NOT NULL,
    superseded_at TIMESTAMPTZ,                -- NULL = current
    content       TEXT NOT NULL,              -- markdown, rendered escaped
    UNIQUE (kind, version)
);
CREATE INDEX policy_version_kind_idx ON policy_version (kind, effective_at DESC);
