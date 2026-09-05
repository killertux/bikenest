-- Rename the application-prefixed helper after the BikesNest product rename.
-- This is forward-only so databases that already applied migration 0020 are
-- upgraded without rewriting migration history.

ALTER FUNCTION bikenest_is_open_at(bigint, text, timestamptz)
    RENAME TO bikesnest_is_open_at;
