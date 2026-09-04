-- 0020: "open now" as one database function, plus the index the recommended
-- sort's security sub-score needs.
--
-- 1) `bikenest_is_open_at(location, timezone, instant)`
--
-- "Is this location open right now?" existed twice: once in the domain
-- (`OpeningHours::status_at`, which answers Open/Closed/Unknown for the
-- details page) and once inlined in the search query, which needs it both as
-- a filter (`open_now=true`) and as a per-row flag on the result card. Two
-- copies of a wall-clock rule with an overnight arm is one copy too many —
-- they drifted once already (the timezone conversion was applied twice).
--
-- The search SQL now calls this function for both the filter and the flag, so
-- the SQL side is a single expression. The domain keeps `status_at` because it
-- answers a different question: it distinguishes "closed" from "hours
-- unknown", which the card's boolean cannot express. `bikenest_is_open_at`
-- is exactly `status_at(...) == Open` (no rows → false, matching Unknown ≠
-- Open); `parking_test.rs` asserts that agreement across same-day, overnight,
-- all-day, DST and no-rows cases.
--
-- Semantics, mirroring `OpeningHours::status_at`:
--   * `all_day` rows are open for the whole of their own day.
--   * A row whose `closes_at <= opens_at` runs past midnight, so it counts
--     twice: on its own day from `opens_at` to midnight, and on the following
--     day until `closes_at`.
--   * Otherwise open on `[opens_at, closes_at)` — left-inclusive.
-- The instant is converted to the location's wall clock exactly once
-- (`at AT TIME ZONE tz` on a `timestamptz`); converting twice would
-- reinterpret an already-naive timestamp and shift the clock by the offset in
-- the wrong direction.
--
-- STABLE (not IMMUTABLE): it reads `opening_hours`. STABLE is what lets the
-- planner call it once per row in a scan instead of treating it as volatile.

CREATE OR REPLACE FUNCTION bikenest_is_open_at(loc_id bigint, tz text, at_instant timestamptz)
RETURNS boolean
LANGUAGE sql
STABLE
PARALLEL SAFE
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM opening_hours oh
        CROSS JOIN (
            SELECT
                EXTRACT(ISODOW FROM l.local_ts)::smallint AS dow,
                EXTRACT(ISODOW FROM l.local_ts - interval '1 day')::smallint AS prev_dow,
                l.local_ts::time AS local_time
            FROM (SELECT at_instant AT TIME ZONE tz AS local_ts) l
        ) lc
        WHERE oh.location_id = loc_id
          AND (
                 (oh.day_of_week = lc.dow AND (
                      oh.all_day
                      OR (oh.opens_at <= lc.local_time AND lc.local_time < oh.closes_at)
                      OR (oh.closes_at <= oh.opens_at AND oh.opens_at <= lc.local_time)
                  ))
              OR (oh.day_of_week = lc.prev_dow
                  AND NOT oh.all_day
                  AND oh.closes_at <= oh.opens_at
                  AND lc.local_time < oh.closes_at)
          )
    )
$$;

COMMENT ON FUNCTION bikenest_is_open_at(bigint, text, timestamptz) IS
    'True when the location is open at that instant, read on its own wall clock. The SQL half of OpeningHours::status_at (== Open).';

-- ---------------------------------------------------------------------------
-- 2) parking_security: confirmed-feature count without a heap visit
-- ---------------------------------------------------------------------------
-- The `Recommended` sort key is now computed in SQL, and its security
-- sub-score needs "how many features are confirmed present" for every
-- candidate row. The primary key is `(location_id, feature_code)`, which
-- locates a location's rows but does not carry `state`, so counting the
-- confirmed ones costs a heap visit per row. This partial index answers the
-- count from the index alone.
--
-- See the note in 0017: on a live database this should be built with
-- CREATE INDEX CONCURRENTLY out of band.
CREATE INDEX parking_security_yes_idx
    ON parking_security (location_id)
    WHERE state = 1;
