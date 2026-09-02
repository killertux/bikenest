-- 0005_accounts: extend the M0 users table with the account lifecycle (§20),
-- plus authentication identities, server-side sessions, verification/reset
-- tokens and granted roles. See plans/m2-accounts-auth.md §3.

-- Extend the M0 users table with the account lifecycle (§20).
ALTER TABLE users
    ADD COLUMN account_state     TEXT NOT NULL DEFAULT 'PENDING_EMAIL_VERIFICATION',
    ADD COLUMN email_verified_at TIMESTAMPTZ,
    ADD COLUMN suspended_at      TIMESTAMPTZ;
-- account_state values: 'PENDING_EMAIL_VERIFICATION' | 'ACTIVE' | 'SUSPENDED' | 'DELETED'
-- (validated in the domain; TEXT keeps it extensible like parking_type).

-- One row per login method (§17): password and google now, future providers later.
CREATE TABLE authentication_identities (
    id                BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id           BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider          TEXT NOT NULL,              -- 'password' | 'google'
    provider_subject  TEXT NOT NULL,              -- password → lower(email); google → `sub`
    credential_hash   TEXT,                       -- argon2id PHC string; NULL for OAuth
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (provider, provider_subject)
);
CREATE INDEX authentication_identities_user_idx ON authentication_identities (user_id);

-- Server-side sessions (§18). The cookie holds the raw id; the DB stores its
-- SHA-256 hash only. `csrf_token` is the per-session synchronizer token.
CREATE TABLE sessions (
    token_hash   TEXT PRIMARY KEY,               -- sha256_hex(raw session id)
    user_id      BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    csrf_token   TEXT NOT NULL,                  -- 32 random bytes, base64url (server-only)
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at   TIMESTAMPTZ NOT NULL,           -- absolute cap (90 days from creation)
    revoked_at   TIMESTAMPTZ
);
CREATE INDEX sessions_user_idx ON sessions (user_id);
CREATE INDEX sessions_expires_idx ON sessions (expires_at);

-- Email verification tokens (§16): registration + change-email share this table.
CREATE TABLE email_verification_tokens (
    token_hash TEXT PRIMARY KEY,                 -- sha256_hex(raw token)
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    email      TEXT NOT NULL,                    -- the address being verified
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,             -- now + 24h
    used_at    TIMESTAMPTZ
);
CREATE INDEX email_verification_tokens_user_idx ON email_verification_tokens (user_id);

-- Password reset tokens (§16): single-use, short-lived.
CREATE TABLE password_reset_tokens (
    token_hash TEXT PRIMARY KEY,                 -- sha256_hex(raw token)
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,             -- now + 1h
    used_at    TIMESTAMPTZ
);
CREATE INDEX password_reset_tokens_user_idx ON password_reset_tokens (user_id);

-- Granted roles (§19). USER is implicit baseline (granted at registration).
CREATE TABLE user_roles (
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role       TEXT NOT NULL,                    -- 'USER' | 'MODERATOR' | 'ADMIN'
    granted_by BIGINT REFERENCES users(id) ON DELETE SET NULL,  -- NULL for bootstrap
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, role)
);
CREATE INDEX user_roles_role_idx ON user_roles (role);
