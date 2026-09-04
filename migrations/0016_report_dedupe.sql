-- One open report per (reporter, target). Without it a single user could file
-- the same report over and over until the daily rate limit finally engaged,
-- flooding the moderation queue with duplicates of one complaint.
--
-- The index is partial: once a report is RESOLVED or DISMISSED the same user
-- may report the same target again (the content may have changed since).

-- Existing data already holds duplicate open reports, and a partial unique
-- index cannot be created over them. Collapse each duplicate group to its
-- oldest row (lowest id) — that is the report the moderation queue has been
-- showing and the one whose audit trail is longest.
-- `reporter_id` is NOT NULL (0010), so the self-join needs no NULL guard.
DELETE FROM report r
USING report r2
WHERE r.reporter_id = r2.reporter_id
  AND r.target_type = r2.target_type
  AND r.target_id   = r2.target_id
  AND r.state  IN ('OPEN', 'UNDER_REVIEW')
  AND r2.state IN ('OPEN', 'UNDER_REVIEW')
  AND r.id > r2.id;

CREATE UNIQUE INDEX report_dedupe_idx
    ON report (reporter_id, target_type, target_id)
    WHERE state IN ('OPEN', 'UNDER_REVIEW');
