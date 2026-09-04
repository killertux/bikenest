# Backups & disaster recovery

> **Goal:** survive a database/media failure or a bad deployment, and restore
> without resurrecting data the user legally asked to delete (§98).
>
> **Targets (defaults, tune with real capacity/risk):** RPO ≤ 15 min (WAL
> archiving), RTO ≤ 60 min. Backup retention **30 days** (daily logical + WAL).
> Restore is **exactly one** command + the post-restore retention re-run.

---

## 1. What to back up

- **Postgres** (the source of truth: accounts, parking, reviews, photos metadata,
  audit events, moderation, exports). Everything else is derivable.
- **Media objects** (the S3 bucket, `S3_BUCKET`: full + thumbnail derivatives
  under `uploads/`, plus the dev dataset under `seed/`). These are **not** in the
  DB — if lost, photo rows dangle (they render as missing). Use the bucket's own
  versioning / replication, or `aws s3 sync` to a second bucket.

Templates, the binary, and migrations are embedded in the image — no backup
needed there.

## 2. Postgres: logical dump + WAL archiving

Logical backup is portable and lets you restore to a different point; WAL
archiving gives a low RPO and point-in-time-recovery.

```bash
# Daily logical backup (cron / systemd timer)
pg_dump -Fc "$DATABASE_URL" > /backups/bikenest-$(date +%F).dump
# Keep 30 days (see retention below), then delete older.

# Enable continuous WAL archiving (postgresql.conf):
#   archive_mode = on
#   archive_command = 'test ! -f /wal/%f && cp %p /wal/%f'
# Ship /wal to durable object storage (S3 etc.), encrypted.
```

Point-in-time-recovery needs the base backup (the latest pg_dump or a base
backup + the WAL segment history). If you want automated PITR, use
`pg_basebackup` (physical) bundled with WAL archiving instead of pg_dump alone;
exactly one of the two is enough for your RPO target.

## 3. Media volume

Snapshot the media volume (or back up the object-store bucket) on the same cadence
as the DB logical dump. If using S3-compatible storage, enable versioning +
cross-region replication; a 30-day version retention covers accidental deletion.

## 4. Encryption & access

- Encrypt backups at rest (the destination bucket/disk encryption, and/or
  `gpg`/`age` the dump before shipping).
- Encrypt transport (TLS to the backup target).
- **Least privilege:** only the backup/restore service and the on-call DBA can
  read backups; access is logged. Backups contain **personal data** (art. 5/32),
  so treat them like production data (this is also an access control for §98).

## 5. Retention & lifecycle

- DB logical dumps: **30 days** (adjust to your audit/legal window; **bold** =
  legal/product approval).
- WAL archive: enough to cover your RPO window, typically 24h of WAL + the base
  backup for PITR.
- Media: covered by versioning (30-day minimum default).

## 6. Restore procedure

```bash
# 1. Stop the app (prevent writes during restore / during the retention sweep).
# 2. Restore the logical dump (or PITR to the target point):
createdb bikenest_restored
pg_restore -d "$DATABASE_URL" /backups/bikenest-<date>.dump

# 3. Restore the media volume from the matching snapshot.
# 4. Re-run the retention job — CRITICAL (§98):
bikenest-web retention   # runs inside the app container
```

**Step 4 is mandatory.** The `retention` command purges expired
sessions/tokens/parked-here/exports and (config-gated) anonymizes inactive
accounts / purges deleted account shells. Because a backup can pre-date a `DELETE`
(request §67, account deletion §98), a naive restore would **resurrect** data the
user already had removed. Re-running retention at the restored truth re-applies the
deletion/anonymization invariants so removed data does not come back into active
production state (§98). Until this runs, the restored instance may briefly show
data a user asked to delete — that is acceptable only if it is immediately
swept and never served to the user.

> **Restore exercise:** at least quarterly (or per your policy), restore the
> latest backup into a sandbox environment, run the retention command, and confirm
> a deleted account stays deleted and a content record references no resurrection.
> Document the exercise (date + operator + result) in your incident/ops log.

## 7. Disaster recovery

- **Backups alone are not DR** (§58). DR = the documented restore procedure
  (this doc) + a reproducible image build (`Dockerfile`) + a written RTO/RPO +
  the sandbox restore exercise. Keep a copy of the image and the backup in a
  **different region/account** than production.
- **Do not** rely on `docker compose up` for a DB-backed recovery — the dev
  stack is not a recovery plan.
- Review the DR posture at the same cadence as the restore exercise.
