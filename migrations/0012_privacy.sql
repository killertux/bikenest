-- §72/§73/§74: privacy requests, exports, consent records, and the
-- anonymize-in-place columns. See plans/m6-privacy.md §2–§3.

-- The privacy-request workflow (§72). Retained after anonymization
-- (user_id → NULL) so the compliance record survives the account.
CREATE TABLE privacy_request (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id      BIGINT REFERENCES users(id) ON DELETE SET NULL,
    kind         TEXT NOT NULL CHECK (kind IN
                    ('access', 'rectification', 'deletion', 'export',
                     'restriction', 'objection', 'consent_withdrawal')),
    state        TEXT NOT NULL DEFAULT 'OPEN'
                 CHECK (state IN ('OPEN', 'IN_PROGRESS', 'COMPLETED', 'DECLINED')),
    details      JSONB NOT NULL DEFAULT '{}'::jsonb,  -- rectification fields / objection reasons
    fulfilled_by BIGINT REFERENCES users(id) ON DELETE SET NULL,
    fulfilled_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX privacy_request_user_idx  ON privacy_request (user_id, created_at DESC);
CREATE INDEX privacy_request_state_idx ON privacy_request (state, created_at);

-- Personal-data export (§73). Payload is stored (assembled synchronously) so
-- the download streams a snapshot, not a re-derivation. Single-use token.
CREATE TABLE personal_data_export (
    id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id        BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    state          TEXT NOT NULL DEFAULT 'READY'
                   CHECK (state IN ('READY', 'DOWNLOADED', 'EXPIRED')),
    token_hash     TEXT NOT NULL,              -- sha256_hex(raw download token)
    payload        JSONB NOT NULL,             -- ExportPayload, schema_version 1
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at     TIMESTAMPTZ NOT NULL,       -- now + 24h (§75)
    downloaded_at  TIMESTAMPTZ
);
CREATE INDEX personal_data_export_user_idx    ON personal_data_export (user_id, created_at DESC);
CREATE INDEX personal_data_export_expiry_idx  ON personal_data_export (expires_at)
    WHERE state = 'READY';

-- Consent records (§69). Empty in the initial release (no consent-based
-- processing — §78); the C6 "consent records" surface is ready for the future.
CREATE TABLE consent_record (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scope      TEXT NOT NULL,                 -- e.g. 'email_marketing', 'cookie_analytics'
    status     TEXT NOT NULL DEFAULT 'GRANTED'
               CHECK (status IN ('GRANTED', 'WITHDRAWN')),
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    withdrawn_at TIMESTAMPTZ
);
CREATE INDEX consent_record_user_idx ON consent_record (user_id, scope);

-- users gains the anonymization clock (§75 "deleted accounts" retention).
ALTER TABLE users ADD COLUMN deleted_at TIMESTAMPTZ;

-- Anonymize-in-place requires the attribution columns to be nullable so the
-- anonymization transaction can SET them NULL while keeping community rows.
-- Change the FK action to SET NULL as the backstop for any future hard-delete.
ALTER TABLE review DROP CONSTRAINT review_author_id_fkey;
ALTER TABLE review ALTER COLUMN author_id DROP NOT NULL;
ALTER TABLE review ADD CONSTRAINT review_author_id_fkey
    FOREIGN KEY (author_id) REFERENCES users(id) ON DELETE SET NULL;

ALTER TABLE verification DROP CONSTRAINT verification_user_id_fkey;
ALTER TABLE verification ALTER COLUMN user_id DROP NOT NULL;
ALTER TABLE verification ADD CONSTRAINT verification_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE SET NULL;

ALTER TABLE parking_proposal DROP CONSTRAINT parking_proposal_proposer_id_fkey;
ALTER TABLE parking_proposal ALTER COLUMN proposer_id DROP NOT NULL;
ALTER TABLE parking_proposal ADD CONSTRAINT parking_proposal_proposer_id_fkey
    FOREIGN KEY (proposer_id) REFERENCES users(id) ON DELETE SET NULL;

ALTER TABLE report DROP CONSTRAINT report_reporter_id_fkey;
ALTER TABLE report ALTER COLUMN reporter_id DROP NOT NULL;
ALTER TABLE report ADD CONSTRAINT report_reporter_id_fkey
    FOREIGN KEY (reporter_id) REFERENCES users(id) ON DELETE SET NULL;
