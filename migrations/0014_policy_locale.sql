-- §70/§71/§102: legal pages are served per locale (pt-BR default, en). A
-- policy version is now keyed (kind, locale, version); the reader falls back
-- to pt-BR when a locale has no current document.
ALTER TABLE policy_version
    ADD COLUMN locale TEXT NOT NULL DEFAULT 'pt-BR'
        CHECK (locale IN ('pt-BR', 'en'));

ALTER TABLE policy_version DROP CONSTRAINT policy_version_kind_version_key;
ALTER TABLE policy_version ADD CONSTRAINT policy_version_kind_locale_version_key
    UNIQUE (kind, locale, version);

DROP INDEX IF EXISTS policy_version_kind_idx;
CREATE INDEX policy_version_kind_locale_idx
    ON policy_version (kind, locale, effective_at DESC);
