-- 0006_audit_events: security, account and role actions (§47).
-- No tokens/passwords/PII beyond actor/target identifiers. `metadata` is JSONB
-- for per-action context.

CREATE TABLE audit_events (
    id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    actor_user_id  BIGINT REFERENCES users(id) ON DELETE SET NULL,  -- NULL = system
    action         TEXT NOT NULL,                  -- 'auth.login' | 'role.granted' | …
    target_type    TEXT NOT NULL,                  -- 'user' | 'session' | 'role'
    target_id      TEXT NOT NULL,
    result         TEXT NOT NULL,                  -- 'success' | 'failure'
    metadata       JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX audit_events_actor_idx  ON audit_events (actor_user_id);
CREATE INDEX audit_events_action_idx ON audit_events (action, created_at DESC);
