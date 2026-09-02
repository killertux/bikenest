# BikeNest

A community-maintained bicycle parking finder. See `REQUIREMENTS.md` (what/how), `PLAN.md`
(milestone plan) and `UI_DESIGN.md` (pages + pointer to the approved visual design in
`design-project/`).

## Status

- [x] **M0** walking skeleton (health/readiness, migrations, real-Postgres test harness, Tailwind pipeline)
- [x] **M1** core search & map (read-only): the full read-only product loop over mock data

### M1 — core search & map (read-only)

- [x] Full parking schema: `parking_location` + PostGIS (GiST), tri-state security attributes, wall-clock opening hours
- [x] Domain model: `ParkingLocation`, cost (free/paid/unknown + price), security tri-state, opening hours with open-now (DST-correct), freshness
- [x] Search: `ST_DWithin` proximity, filters (cost/type/security all-of/open-now), keyset pagination, 5 sorts incl. deterministic Rust-side recommendation scoring
- [x] Pages: P1 home, P2 search (MapLibre map + accessible list + HTMX fragments), P3 details, P7 about — plus E1/E2 error pages
- [x] `seed-mock` dev command: 24 deterministic **Curitiba** locations + photos (Ledger #1/#7)
- [x] `FakeGeocoder` (Curitiba landmarks + deterministic fallback, Ledger #2); MapLibre demo tiles (Ledger #3)
- [x] htmx 4 (+ `hx-boost`, `hx-alpine-compat`) / Alpine / MapLibre vendored locally (no CDN)

### Pulled forward from later milestones

- [x] **Object storage** (from M4): `ObjectStorage` port + `LocalDiskStorage` issuing HMAC-signed,
      expiring `/media/{key}` URLs (S3-presign parity; swapping to S3 later is a wiring change).
      A minimal `parking_photo` table (photos default `APPROVED`); the full upload/validation/EXIF/
      thumbnail/moderation pipeline still lands in M4.
- [x] **Internationalization** (from M7, §102): pt-BR + en, auto-detected from `Accept-Language`
      (fallback pt-BR), overridable via a `lang` cookie set by `GET /lang/{pt-br|en}`. All user-facing
      strings live in a catalog (`crates/web/src/i18n.rs`), not hard-coded in domain/application logic.
- [x] Security-attribute labels are a hardcoded code list (`bikenest_domain::SECURITY_FEATURE_CODES`)
      + i18n labels, not a DB catalog table (so labels are localizable).

- [x] 62 tests green: **domain 18, application 13, infrastructure 22, web 9**

### Foundations (M0)

- [x] Cargo workspace: `domain` / `application` / `infrastructure` / `web` / `test-support` (+ `test-macros`)
- [x] PostgreSQL 17 + PostGIS via Docker Compose
- [x] SQLx migrations (`migrations/`), applied automatically on startup (dev workflow)
- [x] `/healthz` (liveness) and `/readyz` (readiness, distinguishes DB-down from app error)
- [x] Shared-runtime `#[db_test]` harness: one multi-threaded tokio runtime, one migrated pool,
      transaction-per-test with automatic rollback, explicit SAVEPOINT helper
- [x] Tailwind CSS 4.3 pipeline with design tokens from `design-project/colors_and_type.css`

## Run locally

Prerequisites: Rust (stable), Docker, Node (for CSS builds).

### Option A — full stack in Docker

`docker compose up -d` starts the database, the app (auto-recompiling via `cargo watch`) and the
Tailwind CSS watcher. First start compiles the workspace (several minutes); named volumes cache the
cargo target dir, the registry and `node_modules`, so later starts are fast.

```bash
cp .env.example .env          # adjust DB_HOST_PORT if 5432 is taken
docker compose up -d          # db + app (:8080) + css watcher
docker compose logs -f app    # watch it compile & serve

# Seed dev data (once the app is up; production starts empty — §116.1)
docker compose exec app cargo run -q -- seed-mock
```

### Option B — DB in Docker, app on the host

```bash
cp .env.example .env
docker compose up -d db        # PostgreSQL + PostGIS only

# Frontend assets (compiled CSS + vendored JS are committed; only needed when
# changing styles or JS libraries)
npm install
npm run build:assets           # vendor htmx + hx-alpine-compat + alpine + maplibre → web/static/vendor
npm run build:css              # Tailwind 4.3 → web/static/css/app.css

# The DB must be running before `cargo build` — SQLx compile-time query checking
# reads DATABASE_URL from .env.
cargo run -- seed-mock         # mock data (dev only)
cargo run                      # default command; serves on BIND_ADDR (:8080)

curl localhost:8080/healthz    # → 200 ok
curl localhost:8080/readyz     # → 200 {"status":"ready","database":"up"}
```

### Environment

`.env.example` documents all knobs. Notable ones:

- `DATABASE_URL` — Postgres connection (required, read at build time by SQLx).
- `BIND_ADDR` — HTTP bind address (default `0.0.0.0:8080`).
- `MEDIA_ROOT` — object-storage directory (default `<repo>/media`, gitignored).
- `MEDIA_SIGNING_SECRET` — signs the expiring `/media` GET URLs (set a real secret outside dev).

### Reset local data

```bash
docker compose down -v && docker compose up -d    # clean database + fresh volumes
```

## Tests

```bash
cargo test
```

- Domain tests are pure unit tests (opening hours incl. DST, freshness, cost tri-state, type/currency
  validation, scoring neutrality). No database needed.
- `#[db_test]` tests (integration + HTTP) require the compose database running (`docker compose up -d db`)
  — it must be up even to *build*, because SQLx compile-time query checking connects to it (§9).
- Most tests run inside a transaction rolled back at the end. Read-model tests whose queries run on
  *other* pool connections use the committed-fixture pattern instead: rows are tagged with a unique
  `seed_key` marker, committed, asserted against, then deleted by tag (see `parking_test.rs`).

## Layout

```text
crates/domain            pure business concepts (no axum/sqlx/askama)
crates/application       use cases + ports (Geocoder, ParkingSearchReader, ObjectStorage, …); depends on domain only
crates/infrastructure    sqlx persistence, config, probe, FakeGeocoder, LocalDiskStorage, devdata + seeder
crates/web               axum routing, handlers, view models, i18n, Askama templates
crates/test-support      #[db_test] harness, pool fixture, User/Parking builders
crates/test-macros       #[db_test] proc macro
migrations/              SQLx migrations (version-controlled, forward-only)
templates/               Askama layouts/pages/components/partials (workspace root)
web/static/              Tailwind entry + compiled CSS, vendored vendor/ JS, page JS
media/                   object-storage root (dev, gitignored)
design-project/          approved visual design (source of truth)
```

Dependency direction: `domain ← application ← {infrastructure, web}` (see `plans/m0-walking-skeleton.md`).
