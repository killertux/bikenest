# BikeNest

A community-maintained bicycle parking finder. See `REQUIREMENTS.md` (what/how), `PLAN.md`
(milestone plan) and `UI_DESIGN.md` (pages + pointer to the approved visual design in
`design-project/`).

## Status

- [x] **M0** walking skeleton (health/readiness, migrations, real-Postgres test harness, Tailwind pipeline)
- [x] **M1** core search & map (read-only): the full read-only product loop over mock data
- [x] **M2** accounts & authentication: register → verify → log in/out; seeded admin; sessions + CSRF; roles
- [x] **M3** community contributions: verified users add/edit/propose/review/verify/favorite; field-level history + optimistic concurrency; confidence (§106)

### M1 — core search & map (read-only)

- [x] Full parking schema: `parking_location` + PostGIS (GiST), tri-state security attributes, wall-clock opening hours
- [x] Domain model: `ParkingLocation`, cost (free/paid/unknown + price), security tri-state, opening hours with open-now (DST-correct), freshness
- [x] Search: `ST_DWithin` proximity, filters (cost/type/security all-of/open-now), keyset pagination, 5 sorts incl. deterministic Rust-side recommendation scoring
- [x] Pages: P1 home, P2 search (MapLibre map + accessible list + HTMX fragments), P3 details, P7 about — plus E1/E2 error pages
- [x] `seed-mock` dev command: 24 deterministic **Curitiba** locations + photos (Ledger #1/#7)
- [x] `FakeGeocoder` (Curitiba landmarks + deterministic fallback, Ledger #2); MapLibre demo tiles (Ledger #3)
- [x] htmx 4 (+ `hx-boost`, `hx-alpine-compat`) / Alpine / MapLibre vendored locally (no CDN)

### M2 — accounts & authentication

- [x] Schema: `authentication_identities`, `sessions`, `email_verification_tokens`, `password_reset_tokens`, `user_roles`, `audit_events`; `users` carries the account lifecycle (`account_state`, `email_verified_at`, `suspended_at`)
- [x] Domain: `AccountState`, `Role`, `AuthenticationProvider`, `Password`/`PasswordPolicy`, `SessionId`/`CsrfToken`/`VerificationToken` value objects
- [x] Password hashing **argon2id** (OWASP params); server-side sessions (`HttpOnly`/`Secure`/`SameSite=Lax`, SHA-256 hashed at rest); **CSRF** synchronizer token
- [x] Register → verify email (captured fake email) → log in/out; password reset; change password/email
- [x] Google OAuth behind the `AuthenticationProvider` port (`FakeOAuthProvider`, **Ledger #5**)
- [x] `seed-admin` command (**Ledger #10**) + audited `GrantRole`/`RevokeRole`; deny-by-default authorization (§19)
- [x] **Rate limiting** on login/register/reset/resend (§45, **Ledger #6**); no-account-existence leak (§45)

### M3 — community contributions

- [x] Schema: `parking_location` gains `version` (optimistic concurrency) + `creator_id`; `parking_revision` (immutable field-level history, JSONB after-state snapshots); `parking_proposal` (PENDING sensitive changes); `review` + `review_revision` (five-star, one per user per location, history preserved); `verification` (existence/attribute/parked-here, `parked_here` expires); `favorite`
- [x] Domain: `StarRating`/`ReviewBody`/`VerificationKind`/`ExistenceResult`/`AttributeResult`/`ProposalKind`/`ProposalStatus`/`ChangeKind`/`Confidence` + the pure confidence-resolution rule (§106)
- [x] Application: `TimezoneResolver` port + `OfflineTimezoneResolver` (Ledger #16); contribution ports + `ContributionService` (verified gate, §45 rate limits, advisory duplicate detection, §100 version conflicts); `RecommendationExplanation` (§105, same sub-scores as the M1 scorer)
- [x] Infrastructure: `SqlxParkingContributionRepository` (optimistic apply + revision), `SqlxReviewRepository` (recompute aggregate in-tx), `SqlxVerificationRepository` (`DISTINCT ON` latest-per-user), `SqlxFavoriteRepository`, `SqlxContributionHistoryReader`
- [x] Web: D1 add, D2 edit + gated proposal, D3 review, D4 verify/parked-here, favorite toggle; C4 favorites, C5 contributions; `require_verified` gate; P3 gains reviews / confidence / favorite / verification panel / "recommended because…"; HTMX + i18n (en/pt-BR)

### M4 — photos (upload → validate → process → moderate → publish)

- [x] Schema (`0009_photos.sql`): `parking_photo` gains `uploader_id`, `thumbnail_key`, `width`,
      `height`, `processed_at`, `rejection_reason`, `reviewed_by`, `reviewed_at`; `moderation_state`
      default flips `APPROVED → PENDING_REVIEW`; pending-queue + uploader indexes
- [x] Domain: `PhotoModerationState` (`PENDING_REVIEW`/`APPROVED`/`REJECTED`), `PhotoDimensions`,
      upload constants (10 MiB, 20 MP, JPEG q85, 400 px thumb, jpeg/png/webp allowlist)
- [x] Application: `ImageProcessor` + `PhotoRepository` ports, `PhotoService` (`upload_photo`,
      `approve_photo`, `reject_photo`, `list_pending_photos`); verified gate + photo-upload rate
      limits (10/day/user, 20/day/IP); uploads are held `PENDING_REVIEW`; the original is discarded
      after processing (§80 — only processed derivatives are stored)
- [x] Infrastructure: `image`-crate `LocalImageProcessor` (decode → apply EXIF orientation →
      re-encode JPEG → thumbnail, stripping all metadata); `SqlxPhotoRepository` (insert pending /
      approve with position / reject records reason + returns keys / queue oldest-first); the P3
      gallery reader returns `thumbnail_key`
- [x] Web: `POST /parking/{id}/photo` (multipart, verified, CSRF via header); `/moderation/photos`
      queue + approve/reject (HTMX, MODERATOR); P3 gallery thumbnails + lightbox + "Add photo"
      control; moderation i18n (en/pt-BR). **D1 photo-attach deferred** — the P3 path is primary;
      a location can be created first and its photo attached via P3 (same pipeline).

### M5 — moderation & reporting

- [x] Schema (`0010_moderation.sql`, `0011_review_photos.sql`): `report` table (polymorphic
      `target_type`/`target_id`, states `OPEN/UNDER_REVIEW/RESOLVED/DISMISSED`); `parking_photo`
      moderation CHECK widened to accept `HIDDEN`; audit-viewer indexes; `review_photo` table (D3
      photo attach, `PENDING_REVIEW → APPROVED/REJECTED/HIDDEN`)
- [x] Domain: `ReportState`/`ReportTargetType`/`ReportOutcome`, the `REPORT_REASONS` code list +
      `reason_allowed_for` mapping, `ReportDescription` (0..=1000 chars); `PhotoModerationState::Hidden`
- [x] Application: `ModerationService` (submit/claim/resolve report with the **server-side
      self-resolve guard**, hide/restore review/photo, invalidate/restore parking, approve/reject
      proposal, audit viewer, contribution inspection); report rate limits (10/day/user + 20/day/IP);
      `AuthService::suspend_user`/`restore_user` (revokes sessions); `PhotoService` generalized over
      `PhotoTarget { Parking, Review }`
- [x] Infrastructure: `SqlxReportRepository`, `SqlxModerationRepository` (target-existence,
      hide/restore, parking state + `moderation` revision, proposal apply + **supersede** older
      PENDING), `SqlxAuditLogReader` (filter + keyset pagination), review-photo + unified photo queue
- [x] Web: report modal + `POST /reports`; P3 returns **404 for a non-ACTIVE location** to the
      public (moderators see a banner); `/moderation` dashboard, `/moderation/reports`(claim/resolve/
      dismiss), `/moderation/proposals`(approve/reject), hide/restore review/photo/parking;
      `/admin/users/{id}/suspend|restore`, `/admin/users/{id}/contributions`, `/admin/audit`; D3
      review form is multipart (text publishes `ACTIVE`, photos held `PENDING_REVIEW`); i18n (en/pt-BR)

- [x] Beyond the base milestone: every article resolves to the audit trail (`report.*`, `review.*`,
      `photo.*`, `parking.*`, `user.suspended/restored`, `proposal.*`); `PhotoService` hides/restores
      parking-photos for the unified moderation queue.

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

- [x] The full suite is green across **domain / application / infrastructure / web** (M4 adds the
      photo process + repo tests and the upload/moderation HTTP coverage).

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

# Seed an admin user (Ledger #10): set ADMIN_EMAIL/ADMIN_PASSWORD in .env first.
cargo run -- seed-admin

# Versioned legal pages (Ledger #21): upsert the current policies/*.md as new
# policy_version rows (placeholder legal text — requires review, §71).
cargo run -- seed-policies

# Retention job (§75): purge expired sessions/tokens/parked-here/exports +
# orphan media sweep. The two config-gated steps (inactive-anonymize,
# deleted-shell-purge) default off (0) until approved.
cargo run -- retention

cargo run                      # default command; serves on BIND_ADDR (:8080)

curl localhost:8080/healthz    # → 200 ok
curl localhost:8080/readyz     # → 200 {"status":"ready","database":"up"}
```

**Email in dev.** The `EmailProvider` port is selected by `EMAIL_PROVIDER`
(`fake | smtp | resend`; **Ledger #4**). Dev uses **smtp → Mailpit** (already in
docker-compose): `docker compose up -d mailpit`, then open
`http://localhost:8025` to view and audit every sent email. The `fake` provider
instead writes captures to `<MEDIA_ROOT>/outbox/` and `tracing::info!`-logs them.
`/auth/google` uses the `FakeOAuthProvider` stub (**Ledger #5**) — no Google
credentials needed.

### Environment

`.env.example` documents all knobs. Notable ones:

- `DATABASE_URL` — Postgres connection (required, read at build time by SQLx).
- `BIND_ADDR` — HTTP bind address (default `0.0.0.0:8080`).
- `MEDIA_ROOT` — object-storage directory (default `<repo>/media`, gitignored).
- `MEDIA_SIGNING_SECRET` — signs the expiring `/media` GET URLs (set a real secret outside dev).
- `BASE_URL` — builds emailed verification/reset links (default `http://localhost:8080`).
- `EMAIL_PROVIDER` — `fake | smtp | resend` (default `fake`). Dev uses `smtp`.
- `EMAIL_FROM` — envelope sender for every email.
- `SMTP_HOST` / `SMTP_PORT` / `SMTP_USERNAME` / `SMTP_PASSWORD` / `SMTP_TLS` — SMTP backend (Mailpit: `localhost:1025`, no TLS/auth).
- `RESEND_API_KEY` / `RESEND_FROM` — Resend API backend.
- `ADMIN_EMAIL` / `ADMIN_PASSWORD` — `seed-admin` bootstrap (Ledger #10; password must be 8+ chars).
- `FAKE_OAUTH_EMAIL` / `FAKE_OAUTH_SUB` — deterministic `FakeOAuthProvider` dev identity (Ledger #5).

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
