# Deployment (§58–§63, §116.1)

> **What this covers:** getting the `bikenest-web` server into production. The
> runtime queries are **not** compile-time checked (M8), so the Docker image
> builds with **no database** — `cargo build` is self-contained. Everything else
> (secrets, providers, TLS, migration, health) is env-driven and documented here.

---

## 1. Build the image

```bash
docker build -t bikenest:$(git rev-parse --short HEAD) .
```

The multi-stage `Dockerfile` (`rust:1.95` builder → `debian:bookworm-slim`) bakes
the release binary and `web/static/`. Templates and migrations are **embedded**
(Askama / `sqlx::migrate!`), so nothing else is copied. Uploaded media lives on a
mounted volume (`MEDIA_ROOT`, default `/app/media`) — it is **not** in the image.

Build is reproducible because `Cargo.lock` is committed and the toolchain is
pinned by the base image tag. No `DATABASE_URL`, no offline cache, no build-time
DB.

## 2. Required environment

All knobs are documented in `.env.example`; production sets them as real secrets
(or a secret manager mounted as env). The table below is what matters most.

| Variable | Notes |
|---|---|
| `DATABASE_URL` | Postgres DSN. Example `postgres://user:pass@db:5432/bikenest` |
| `BIND_ADDR` | default `0.0.0.0:8080` |
| `BASE_URL` | the public origin, e.g. `https://bikenest.example.com` — builds links + canonical URLs. **Must be reachable** |
| `MEDIA_SIGNING_SECRET` | signs the expiring `/media` GET URLs. Set a long random secret; rotate deliberately |
| `MEDIA_ROOT` | object-storage directory (default `/app/media`) — the mounted volume |
| `TLS_ON` | set `true` to emit HSTS behind a real TLS terminator |
| `VALKEY_URL` | **Rate limiter** (Ledger #6) single node, e.g. `valkey://valkey:6379`. Shared across auth/photo/contribution/moderation, survives restarts, aggregates across instances |
| `VALKEY_CLUSTER_URLS` | comma-separated node URLs → **cluster** mode (wins over `VALKEY_URL`) |
| `RATE_LIMIT_FAIL_OPEN` | `true` (default) → a ValKey outage **allows** requests (goes fail-open); `false` → **denies** (429s the rate-limited endpoints) |
| `CSP_TILE_HOSTS` / `CSP_GEOCODE_HOSTS` | origins allowed by the strict CSP for map tiles / geocoding |
| `APP_ENV` | `production` → JSON structured logs (machine-parseable, forward to a log aggregator) |
| `EMAIL_PROVIDER` | `smtp` or `resend` in production (not `fake`) |
| `SMTP_*` / `RESEND_API_KEY` / `RESEND_FROM` | the chosen email backend |
| `ADMIN_EMAIL` / `ADMIN_PASSWORD` | `seed-admin` bootstrap (run once) |
| `REC_*`, `FRESHNESS_*`, `PHOTO_*`, `MOD_*`, `RETENTION_*` | tuning constants (see `.env.example`) |

**Never** put secrets in the image; the `.dockerignore` excludes `.env*`.

## 3. TLS, reverse proxy, health checks

Terminate TLS at a reverse proxy / LB (Caddy, Traefik, nginx, or a cloud LB) and
set `TLS_ON=true` so the app emits `Strict-Transport-Security`. The app itself
listens plaintext on `BIND_ADDR`.

Health/readiness endpoints:

- `GET /healthz` → liveness (process up). Wire to the LB's health check.
- `GET /readyz` → readiness (DB reachable + migrations applied). Wire to the LB's
  readiness gate so `readyz` fails during a migration before rollout completes.

## 4. Migrations

The server runs `sqlx::migrate!` **on startup** (the default subcommand `serve`).
Migrations are **forward-only** (`sqlx` records applied versions). This means:

- **Deploy = run the new image.** The migration runs before the server accepts
  traffic (`readyz` gates until migrations are applied).
- **Rollback = redeploy the previous image.** Because migrations are
  forward-only, a *rollback* is a restore of the previous release, **not** an
  automated down-migration. New columns introduced by the rolled-back release
  become harmless (ignored) but remain in the schema.

> If a release must be undone and the schema is incompatible, restore the
> pre-release data backup and re-run the old image — see `docs/backups.md`.

## 5. Providers

The map/tile, geocoder, email, OAuth and object-storage integrations are
currently **dev impls** (see `PENDING_FOR_PRODUCTION.md` §A–§B). Wire the real
providers there; the env surface is already in place.

## 5b. Rate limiter (ValKey, Ledger #6)

The rate limiter is a sliding-window counter shared by auth, photo, contribution
and moderation. Dev uses an in-memory limiter; production should run ValKey
(Redis-compatible), which aggregates limits across instances and survives
restarts. The app picks the backend from env (no code change):

- no `VALKEY_URL`/`VALKEY_CLUSTER_URLS` → in-memory (default, dev/test).
- `VALKEY_URL` → `ValKeyRateLimiter::single` (one node).
- `VALKEY_CLUSTER_URLS` → `ValKeyRateLimiter::cluster` (a real cluster; the
  `redis-rs` `ClusterClient` auto-discovers nodes and routes each key to its
  owning slot). Cluster wins over `VALKEY_URL`.

**Atomicity:** each `check` runs a Lua script against a ValKey sorted set
(`ZREMRANGEBYSCORE` → `ZCARD` → `ZADD`), so the trim+count+record is atomic and
correct under concurrency, including in cluster mode (the script touches a
single key, so it stays within one hash slot).

**Failure mode — fail open by default.** `Check` on a ValKey outage returns
*allow* and logs a `warn!` (`RATE_LIMIT_FAIL_OPEN=true`), so a ValKey outage
degrades brute-force protection without taking the site down (the application
maps any `RateLimitError` to 429 — fail closed — which would 429 every
rate-limited endpoint during an outage). Set `RATE_LIMIT_FAIL_OPEN=false` to
fail closed instead (stricter, but an outage 429s auth/photo/moderation).

**Docker compose:** the dev stack runs a single-node ValKey
(`docker-compose.yml`, `valkey` service, wired as `VALKEY_URL`). For cluster
mode use `docker-compose.valkey-cluster.yml` (a cluster-enabled node covering
all slots — portable on Docker Desktop for Mac; see the file header for a
multi-node variant).


## 6. Rolling deploy + rollback

1. Build the new image (tagged with the commit SHA).
2. Push to the registry; deploy the new image to one instance.
3. Wait for `readyz` to go green on that instance (migrations applied).
4. Drain the old instance; promote the new one.
5. On failure: stop the rollout, redeploy the **previous** image tag, and if the
   schema is incompatible restore the pre-release backup (see §4/`docs/backups.md`).

## 7. Logging & retention

- `APP_ENV=production` → JSON structured logs (one line per request via
  `TraceLayer`: method/path/status/latency; **headers never logged**, so no
  cookie/token/PII). PII-free `info`/`warn` events at key boundaries.
- Forward stdout/stderr to a log driver (Docker/CloudWatch/journald/Syslog).
- Retain per your audit/incident needs (default: 30 days, forward to an archive
  if longer retention is required).
- Separate **audit events** (the `audit_events` table, `action` codes like
  `photo.*`, `report.*`, `privacy.*`, `retention.*`) from diagnostic logs — audit
  rows are queryable and protected (see `docs/incident-response.md`).
