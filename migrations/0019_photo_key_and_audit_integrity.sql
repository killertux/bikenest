-- 0019: make two invariants the database's job rather than the code's.
--
-- 1) A photo row always names a stored object.
-- 2) `audit_events` is append-only, with two named exceptions.

-- ---------------------------------------------------------------------------
-- 1) storage_key is never empty
-- ---------------------------------------------------------------------------
-- The upload path used to insert a row with `storage_key = ''` purely to mint
-- the id its keys were derived from, then patch the keys in a second
-- statement. A crash between the two left a PENDING_REVIEW row pointing at no
-- object, which renders as a broken image in the moderation queue forever and
-- is invisible to the retention orphan sweep (the sweep looks for objects with
-- no row, not rows with no object). The keys are now minted from a random id
-- before anything is written, so the row is inserted once, complete — and this
-- CHECK is what keeps it that way.
--
-- The development database held no such rows when this migration was written
-- (`SELECT count(*) ... WHERE storage_key = ''` returned 0 for both tables).
-- The deletes below are for any environment where a crashed upload did leave
-- one: such a row is unusable by definition, so dropping it is the only
-- possible repair, and it must happen before the constraint is added or the
-- migration would fail on that environment instead.
DELETE FROM parking_photo WHERE storage_key = '';
DELETE FROM review_photo  WHERE storage_key = '';

ALTER TABLE parking_photo
    ADD CONSTRAINT parking_photo_storage_key_nonempty CHECK (storage_key <> '');
ALTER TABLE review_photo
    ADD CONSTRAINT review_photo_storage_key_nonempty CHECK (storage_key <> '');

-- ---------------------------------------------------------------------------
-- 2) audit_events is append-only
-- ---------------------------------------------------------------------------
-- An audit trail that can be edited is not evidence. Nothing in the
-- application updates or deletes an audit row except the two operations below,
-- so the table refuses both by default.
--
-- Two mutations are legitimate, and each announces itself by setting
-- `app.audit_purge` for its own transaction (`SET LOCAL`, so it cannot leak to
-- the next statement on a pooled connection):
--
--   * the LGPD erasure scrub (`privacy/anonymize.rs`), which de-attributes the
--     rows of a deleted account, and
--   * the retention purge, via `purge_audit_events_before()` below.
--
-- Independently of the setting, one narrow UPDATE is always allowed: nulling
-- `actor_user_id` while every other column stays byte-identical. That is what
-- the `actor_user_id REFERENCES users(id) ON DELETE SET NULL` foreign key does
-- on its own whenever an account shell is hard-purged, and blocking it would
-- turn `DELETE FROM users` into an error.
--
-- This is not tamper-proofing against someone with SQL access — a GUC is not a
-- permission — it is a guarantee that no code path mutates the trail by
-- accident, and that any path which does has to say so in the diff.
CREATE FUNCTION audit_events_immutable() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF coalesce(current_setting('app.audit_purge', true), '') = 'on' THEN
        RETURN CASE TG_OP WHEN 'DELETE' THEN OLD ELSE NEW END;
    END IF;

    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION
            'audit_events is append-only: DELETE requires purge_audit_events_before()';
    END IF;

    IF NEW.id            IS DISTINCT FROM OLD.id
       OR NEW.action     IS DISTINCT FROM OLD.action
       OR NEW.target_type IS DISTINCT FROM OLD.target_type
       OR NEW.target_id  IS DISTINCT FROM OLD.target_id
       OR NEW.result     IS DISTINCT FROM OLD.result
       OR NEW.metadata   IS DISTINCT FROM OLD.metadata
       OR NEW.created_at IS DISTINCT FROM OLD.created_at
       OR NEW.actor_user_id IS NOT NULL
    THEN
        RAISE EXCEPTION
            'audit_events is append-only: only actor_user_id may be nulled (erasure)';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER audit_events_append_only
    BEFORE UPDATE OR DELETE ON audit_events
    FOR EACH ROW EXECUTE FUNCTION audit_events_immutable();

-- The sanctioned purge (docs/retention-policy.md: audit events are kept five
-- years). SECURITY DEFINER so it can be granted to the application role
-- without granting a bare DELETE on the table; the `SET LOCAL` is scoped to
-- the function's own transaction.
CREATE FUNCTION purge_audit_events_before(cutoff TIMESTAMPTZ) RETURNS BIGINT
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    removed BIGINT;
BEGIN
    SET LOCAL app.audit_purge = 'on';
    DELETE FROM audit_events WHERE created_at < cutoff;
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END;
$$;

-- ---------------------------------------------------------------------------
-- 3) audit_events.target_id stays TEXT — deliberately
-- ---------------------------------------------------------------------------
-- `report.target_id` is BIGINT and `audit_events.target_id` is TEXT, which
-- looks like drift. It is not: the audit column is polymorphic over
-- `target_type` and genuinely holds non-numeric values. In the development
-- database, several hundred `target_type = 'user'` rows hold an *email
-- address* (a failed login has no user id to record — see
-- `AuthService::login`) and the `target_type = 'system'` rows hold the literal
-- 'retention'. `ALTER COLUMN ... TYPE bigint` would fail on those rows, and
-- succeeding would mean losing the identifier a failed-login investigation
-- needs.
--
-- The email is personal data, so `privacy/anonymize.rs` rewrites it to the
-- account's anonymized form on deletion, in the same transaction as the rest
-- of the erasure.
COMMENT ON COLUMN audit_events.target_id IS
    'Polymorphic over target_type: a row id for most types, an email address '
    'for a failed login (no user id exists), a literal name for system events. '
    'Stays TEXT for that reason; anonymization rewrites the email form.';
