-- The language each account reads (REQUIREMENTS §12).
--
-- Transactional email is sent by a background job, where there is no request
-- and therefore no `Accept-Language` header and no `lang` cookie to read: the
-- only way to write to someone in their own language is to have stored it.
-- Set at registration from the locale the signup page was rendered in, and
-- updated whenever a signed-in user uses the header language toggle.
--
-- The default is pt-BR, matching the app's own fallback, so every existing row
-- keeps the language it has been receiving mail in. The CHECK holds the column
-- to the two codes `LocaleCode::as_str()` emits.

ALTER TABLE users
    ADD COLUMN locale TEXT NOT NULL DEFAULT 'pt-BR'
    CHECK (locale IN ('pt-BR', 'en'));
