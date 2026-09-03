# Deployment

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
(Askama / `sqlx::migrate!`), so nothing else is copied. Uploaded media lives in an
**S3-compatible bucket** (MinIO in dev), not the image; the bucket is the
configured store (`S3_*` env). Media is served via **direct S3 presigned GET
URLs** (the browser hits the bucket; the app is not a media proxy).

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
| `MEDIA_ROOT` | legacy local media directory (default `/app/media`) — only used by the retention orphan-media sweep; with direct S3 presign the objects live in the bucket |
| `S3_ENDPOINT` | **Object storage** (Ledger #7): the S3-compatible endpoint (default `http://localhost:9000` → MinIO). Set `S3_ENDPOINT=` (empty) for the standard AWS endpoint |
| `S3_REGION` / `S3_BUCKET` | coverage: region (default `us-east-1`) + bucket name (default `bikenest`) |
| `S3_ACCESS_KEY_ID` / `S3_SECRET_ACCESS_KEY` | S3 credentials (default MinIO `minioadmin`; **required** real creds in prod) |
| `TLS_ON` | set `true` to emit HSTS behind a real TLS terminator |
| `VALKEY_URL` | **Rate limiter** (Ledger #6) single node, e.g. `valkey://valkey:6379`. Shared across auth/photo/contribution/moderation, survives restarts, aggregates across instances |
| `VALKEY_CLUSTER_URLS` | comma-separated node URLs → **cluster** mode (wins over `VALKEY_URL`) |
| `RATE_LIMIT_FAIL_OPEN` | `true` (default) → a ValKey outage **allows** requests (goes fail-open); `false` → **denies** (429s the rate-limited endpoints) |
| `JOBS_ENABLED` | **Background job queue** (M9): `true` (default) spawns an in-process worker that claims/run/retries `background_job` rows; `false` for web-only instances |
| `JOBS_POLL_INTERVAL_MS` / `JOBS_BATCH_SIZE` / `JOBS_LEASE_TTL_MS` | queue poll cadence, batch size, and lease length (defaults 5000 / 4 / 600000) |
| `JOBS_MAX_ATTEMPTS` / `JOBS_BACKOFF_BASE_MS` | retry budget (default 5) and exponential-backoff base (default 2000) before dead-letter |
| `JOBS_HISTORY_RETENTION_DAYS` | `jobs.gc` deletes `succeeded`/`failed` rows older than this (default 7) |
| `CSP_TILE_HOSTS` / `CSP_GEOCODE_HOSTS` | origins allowed by the strict CSP for map tiles / geocoding |
| `APP_ENV` | `production` → JSON structured logs (machine-parseable, forward to a log aggregator) |
| `GEOCODER` | **Geocoder** (Ledger #2): `mapbox` \| `fake` (default `fake`). `mapbox` sends the query to Mapbox server-side (§77/§83) |
| `MAPBOX_ACCESS_TOKEN` | Mapbox token; required when `GEOCODER=mapbox` (missing token falls back to `fake`) |
| `MAP_STYLE_URL` | **Basemap** (Ledger #3): style URL; default MapLibre demo tiles |
| `MAPBOX_MAP_ACCESS_TOKEN` | **Basemap** public Mapbox token (client-side); falls back to `MAPBOX_ACCESS_TOKEN` if unset; only loaded when the style is Mapbox-based |
| `EMAIL_PROVIDER` | `smtp` or `resend` in production (not `fake`) |
| `SMTP_*` / `RESEND_API_KEY` / `RESEND_FROM` | the chosen email backend |
| `ADMIN_EMAIL` / `ADMIN_PASSWORD` | `seed-admin` bootstrap (run once) |
| `POLICY_OPERATOR_NAME` / `POLICY_OPERATOR_CNPJ` / `POLICY_OPERATOR_ADDRESS` / `POLICY_CONTACT_EMAIL` | **Legal pages** (Ledger #21, §70): the controller's legal name, CNPJ, registered address and privacy contact e-mail, substituted into `policies/*.md` by `seed-policies`. The seeder refuses to run with any of them unset |
| `POLICY_VERSION` / `POLICY_EFFECTIVE_AT` | version label + effective date of the policy text being seeded; bump the version whenever `policies/*.md` change |
| `DELETED_ACCOUNT_PURGE_AFTER_DAYS` | `30` in production (decision, `docs/retention-policy.md`); `INACTIVE_ACCOUNT_ANONYMIZE_AFTER_DAYS` stays `0` |
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

The map/tile, geocoder, email, OAuth and object-storage integrations
are selected at wiring time from environment variables; the dev fakes that
remain (Google OAuth) are documented below and must be replaced before launch.

**Geocoder (Ledger #2).** Selectable at wiring time with `GEOCODER`
(`mapbox` | `fake`, default `fake`):

- `fake` — deterministic dev geocoder (landmark table + hashed jitter).
- `mapbox` — real `MapboxGeocoder` calling the Mapbox Geocoding API
  (hosted, OSM-derived; §83). Requires `MAPBOX_ACCESS_TOKEN`; if the token is
  missing it logs and falls back to `fake`.

  **§77 boundary:** the query is sent **server-side** with only the free-text
  destination — no account identity, cookie, or client IP crosses to Mapbox
  (see `docs/provider-transfer-inventory.md`). A Mapbox error is **graceful**: the
  search page shows a "location service unavailable" message rather than a 500.
  Terms of service, attribution, rate limits and the **provider contract / DPA /
  international-transfer review** apply —
  Mapbox is a paid hosted SaaS (free tier ≈100k geocode/mo); self-hosted Photon
  (OSM) is the no-cost, no-external-transfer alternative if preferred.

**Object storage (Ledger #7).** Media is stored in an S3-compatible bucket
(MinIO in dev, AWS/S3/R2/B2 in prod; `S3_*` env) and served via **direct S3
presigned GET URLs** — the browser hits the bucket and S3's SigV4 signature
authorizes the read (no app-side proxy, no app signing secret). Selectable by
`S3_ENDPOINT`/`S3_BUCKET`; defaults target the compose MinIO.

**Email (Ledger #4) — done in code.** Provider is selected by `EMAIL_PROVIDER`
(`fake` | `smtp` | `resend`, default `fake`; dev uses `smtp` → Mailpit). For
production set `EMAIL_PROVIDER=resend` + `RESEND_API_KEY`/`RESEND_FROM`, or
`smtp` + `SMTP_*`. Only the production relay/ESP credentials remain (ops).

**Other providers (tiles, Google OAuth) still use dev impls —**
**tiles are now configurable** (Ledger #3):

- **Tiles / basemap.** `MAP_STYLE_URL` (default MapLibre demo tiles) reaches the
  browser via `<body data-*>` (CSP-safe — no inline script) and is read by
  `search.js` / `details-map.js`. For production set a real basemap: a Mapbox
  style (`mapbox://styles/<user>/<style>` + a public `MAPBOX_MAP_ACCESS_TOKEN`,
  or the HTTPS styles URL), or a self-hosted vector style (Protomaps PMTiles /
  OpenFreeMap — free, no per-request cost, no external transfer). Attribution is
  rendered by MapLibre's attribution control; a hosted provider's ToS /
  attribution / usage limits + DPA (§C) still apply.


## 5b. Rate limiter (ValKey)

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


## 5c. Background jobs (M9)

The app ships a **pure-PostgreSQL job queue** — no broker. A `background_job`
table stores durable one-shot + recurring work; an **in-process worker task**
(started when `JOBS_ENABLED=true`, the default) claims due jobs with
`FOR UPDATE SKIP LOCKED`, runs their handler, and records the outcome. All job
times are UTC.

- **Recurring** jobs never go terminal: on success the worker recomputes
  `run_at` from `schedule` (`{"every_seconds":N}` or a UTC `{"cron":"…"}`) and
  resets the row to `pending`.
- **Retries** use exponential backoff + jitter; after `JOBS_MAX_ATTEMPTS` a job
  is dead-lettered to `failed` (kept with `last_error` for inspection).
- **`jobs.gc`** (itself a recurring job) deletes `succeeded`/`failed` rows older
  than `JOBS_HISTORY_RETENTION_DAYS` (default 7).
- **At-least-once**: a worker crash leaves the job leasable; it is re-claimed
  after the lease. Handlers must be idempotent.
- On a multi-instance deploy each instance runs its own worker; claims are safe
  because `SKIP LOCKED` assigns disjoint rows. `JOBS_ENABLED=false` keeps an
  instance web-only (no worker).

The always-on recurring jobs (`retention`, `jobs.gc`) are bootstrapped by the
worker at startup (idempotent via a stable `idempotency_key`), so no manual
seeding is required. The legacy `cargo run -- retention` subcommand still works
as a manual escape hatch.

## 6. Rolling deploy + rollback

1. Build the new image (tagged with the commit SHA).
2. Push to the registry; deploy the new image to one instance.
3. Wait for `readyz` to go green on that instance (migrations applied).
4. Drain the old instance; promote the new one.
5. On failure: stop the rollout, redeploy the **previous** image tag, and if the
   schema is incompatible restore the pre-release backup (see §4/`docs/backups.md`).

## 6a. Legal pages (privacy / terms / cookies)

The versioned legal pages are stored in `policy_version` and seeded from
`policies/{privacy,terms,cookies}.{pt-BR,en}.md`:

1. Set `POLICY_OPERATOR_NAME`, `POLICY_OPERATOR_CNPJ`, `POLICY_OPERATOR_ADDRESS`
   and `POLICY_CONTACT_EMAIL` (the privacy inbox must be monitored — rights
   requests and takedown notices arrive there).
2. Set `POLICY_VERSION` (e.g. `2026-09-03.1`) and `POLICY_EFFECTIVE_AT`.
3. Run `bikenest-web seed-policies` once per release that changes the text. It is
   idempotent per `(kind, locale, version)`; a new version supersedes the current
   one and the old text stays reachable at `/{privacy,terms,cookies}/versions`.
4. Material changes must be announced to users (e-mail or in-app notice) before
   `POLICY_EFFECTIVE_AT` — the policies promise that.

Review status of the text itself: `docs/legal-review.md`.

## 7. Logging & retention

- `APP_ENV=production` → JSON structured logs (one line per request via
  `TraceLayer`: method/path/status/latency; **headers never logged**, so no
  cookie/token/PII). PII-free `info`/`warn` events at key boundaries.
- Forward stdout/stderr to a log driver (Docker/CloudWatch/journald/Syslog).
- **Access logs: keep 6 months.** As a Brazilian company operating an internet
  application, art. 15 of the Marco Civil da Internet (Lei 12.965/2014) requires
  the *registros de acesso a aplicações* — date/time of access + the client IP —
  to be kept for **6 months** under confidentiality in a controlled environment.
  The app's own request logs do not include the client IP, so satisfy this at
  the reverse proxy / LB access log (or the hosting provider's equivalent):
  retain those logs for 6 months, access-controlled, then delete. Everything
  else (diagnostic logs) can stay at ~30 days. The privacy policy states the
  6-month period; keep them aligned (`docs/retention-policy.md`).
- Separate **audit events** (the `audit_events` table, `action` codes like
  `photo.*`, `report.*`, `privacy.*`, `retention.*`) from diagnostic logs — audit
  rows are queryable and protected (see `docs/incident-response.md`).
