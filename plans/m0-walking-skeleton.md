# M0 — Walking Skeleton: Detailed Implementation Plan

> Milestone plan derived from `PLAN.md` (M0) and `REQUIREMENTS.md`. Parent plan: `PLAN.md`.
> **Goal:** prove the architecture and the dev/test loop end to end before building any user-facing feature.
>
> **Done means:** `docker compose up -d` → `cargo run` serves; `/healthz` returns 200; `/readyz` distinguishes "DB down" from "app error"; `cargo test` runs against real Postgres with rollback isolation; a fresh clone can run everything from the README alone; Tailwind 4.3 pipeline compiles the design tokens.

---

## 1. Scope of M0

**In scope:**

1. Cargo workspace with the crate decomposition below (Clean Architecture boundaries, §3/§4).
2. `docker-compose.yml`: PostgreSQL 17 + PostGIS, named volume, health checks.
3. Migration tooling (`sqlx migrate`) + migration 0001: enable PostGIS extension, minimal `users` table.
4. Configuration loader (env-based), `.env.example`, README onboarding.
5. Axum HTTP server with `/healthz` (liveness) and `/readyz` (readiness with real DB ping) — §87.
6. `test-support` crate: database fixture, transaction-per-test + rollback, SAVEPOINT helper, first builders.
7. One domain test + one integration test against real PostgreSQL.
8. Frontend tooling: Tailwind CSS 4.3 pipeline (CSS entry with `@import "tailwindcss"` + `@theme` mapping `design-project/colors_and_type.css` tokens), a minimal base Askama layout, and E1/E2-style error pages proving the template stack.
9. Structured logging (tracing) — minimal, expanded in M7.

**Explicitly out of scope (later milestones):** all domain features (search, auth, contributions…), real geocoding/email/OAuth/storage, i18n catalog, security headers/CSP, E2E browser tests. No Ledger mock/fake entries arise in M0 (Clock is a trivial real impl if needed).

---

## 2. Cargo workspace structure

```text
bikenest/
├── Cargo.toml                  # workspace root
├── docker-compose.yml
├── .env.example
├── README.md
├── plans/
├── design-project/             # approved visual design (source of truth, not compiled)
├── migrations/                 # sqlx migrate directory (workspace-level, shared)
├── templates/                  # Askama templates (owned by web crate, lives at root for tooling)
│   ├── layouts/base.html
│   └── pages/error.html
├── web/
│   ├── static/
│   │   ├── css/input.css       # Tailwind entry
│   │   └── css/app.css         # compiled (generated, git-ignored initially — commit for simplicity)
│   └── assets/…                # built assets served by axum
└── crates/
    ├── domain/                 # pure business concepts. NO axum/sqlx/askama deps.
    ├── application/            # use cases + ports. Depends on domain only.
    ├── infrastructure/         # persistence (sqlx), config, clock. Depends on domain+application.
    ├── web/                    # axum handlers, askama templates, routing, state. Depends on application+infrastructure.
    └── test-support/           # db fixture, builders, SAVEPOINT helper (dev-only usage)
```

### Dependency direction (must be respected, §113)

```text
domain  ←  application  ←  infrastructure
                ↑                 ↑
                └────── web ───────┘
test-support ── depends on infrastructure (for the pool/fixture) and domain
```

- `domain`: no external async/web/db dependencies (only `serde` where genuinely needed — avoid even that in M0 if unused).
- `application`: defines **ports** (traits) for infrastructure (in M0: `Clock`, `HealthCheck`/unit-of-work access to the pool is via an injected `sqlx::PgPool` handle owned by infrastructure and passed to the web layer as state — application does not depend on sqlx yet; first real port arrives in M1 with geocoding).
- `infrastructure`: sqlx, tokio, config loading; implements ports.
- `web`: axum, askama, tower-http; composes everything; owns `AppState`.
- `test-support`: not published; only used under `[dev-dependencies]`.

Note on pragmatism: in M0 the application crate contains the `ReadinessCheck` use case (is the DB reachable?) which is expressed against a port (`DbStatus` probe) implemented by infrastructure with sqlx. This keeps the dependency rule honest from day one.

---

## 3. Key decisions (decided → implementation → reasoning)

| Area | Decision | Reasoning / tradeoff |
|---|---|---|
| Primary keys | `BIGINT GENERATED ALWAYS AS IDENTITY` for internal tables; `UUIDv7`-style (`uuid v7` via `uuid` crate) where external exposure/guessability matters later (M1+). `users.id` = identity PK in M0. | Simple, ordered, compact; UUIDs deferred to tables exposed in URLs. |
| Timestamps | `TIMESTAMPTZ`, UTC always (`chrono`, `DateTime<Utc>`); rendered per-viewer timezone at the edge (later). §8. | Matches requirement verbatim. |
| Timezone | Stored per-record (M1: `parking_location.timezone`); M0 has none. | — |
| Migrations | `sqlx-cli`-compatible `migrations/` dir at workspace root, run by the binary on startup in dev (`sqlx::migrate!` embedded) + `cargo sqlx` for dev workflow; production runs the same embedded migrations explicitly on deploy (M7 decides gate). §10. | Reproducible, no runtime schema generation. |
| Rollback | Forward-only migrations; rollback = new migration. Documented. | Practical default; per §10 "where practical". |
| Web framework | `axum` (latest), `tokio` runtime. | De-facto standard in the Rust ecosystem; fits tower middleware model. |
| Error handling | thiserror for typed errors per crate; web layer maps them to HTTP; no unwrap in request paths. §85. | — |
| Logging | `tracing` + `tracing-subscriber` (env-filter, JSON optional later). No sensitive data logged (§86). | Cheap to add now. |
| Styling | Tailwind CSS 4.3 via its CLI (`@tailwindcss/cli`), CSS-first config: `@import "tailwindcss"` + `@theme` block porting `design-project/colors_and_type.css` tokens. Compiled CSS committed under `web/static/css/`. | Per REQUIREMENTS §12 and UI_DESIGN.md. |
| Config | `figment` or hand-rolled env loader. **Decision: hand-rolled `Config::from_env()`** (DATABASE_URL, BIND_ADDR, RUST_LOG) to keep deps minimal in M0; revisit if it grows. | Fewer deps; config surface is tiny. |
| Test DB | Tests require `DATABASE_URL` (or `TEST_DATABASE_URL`) pointing at the compose Postgres; `test-support` migrates once (static `OnceLock`), then per-test `BEGIN` … `ROLLBACK`; SAVEPOINT via `sqlx::Acquire` + `Transaction::begin` on a connection inside the test tx (sqlx nests via savepoints automatically when beginning a transaction on a connection that already has one — we expose this explicitly). §49–§51. | Matches required patterns exactly. |
| Test runtime | **Custom `#[db_test]` macro** (decided during implementation). Every `#[tokio::test]` would create its own runtime, and each would need its own pool + migration run. Instead, `#[db_test]` (in `crates/test-macros`) rewrites the test into a plain `#[test]` executed by `run_db_test` on **one shared multi-threaded tokio runtime** with **one shared migrated pool**, providing the test a `&mut TestTx` (an open transaction, rolled back when the test ends). Nested application transactions use an explicit `SAVEPOINT` helper (`Savepoint::commit`/`rollback`), avoiding sqlx's nested-`Transaction` lifetime problems. | Deterministic, fast, matches §49–§51 exactly. |
| Tailwind version pin | `tailwindcss@^4.3` + `@tailwindcss/cli@^4.3` in `package.json` (npm), build script `npm run build:css`. | Node is available; no CDN. |

---

## 4. Database schema — migration 0001

```sql
-- migrations/0001_init.sql
CREATE EXTENSION IF NOT EXISTS postgis;

CREATE TABLE users (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    email         TEXT NOT NULL,
    display_name  TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX idx_users_email ON users (lower(email));
```

Minimal `users` table only (per parent plan); the full auth schema (`authentication_identities`, `sessions`, tokens) arrives in M2. PostGIS is enabled here so the spatial baseline is proven early.

Indexing/FK/deletion strategies are documented in the parent plan as they materialize per milestone.

---

## 5. Health & readiness (§87)

- `GET /healthz` → `200 OK` "ok" — pure liveness, no dependencies touched.
- `GET /readyz` → checks the DB via the application use case `CheckReadiness`:
  - DB reachable → `200` with JSON/HTML `{"status":"ready","database":"up"}`
  - DB unreachable → `503` with `{"status":"degraded","database":"down"}` — **must distinguish DB-down from app-error** (app-error → `500` with generic body).
- Implemented as: web handler → application use case (port `DatabaseProbe`) → infrastructure impl (`sqlx` `SELECT 1` with timeout).

---

## 6. Frontend tooling (M0 scope)

- `package.json` with `tailwindcss@^4.3`, `@tailwindcss/cli@^4.3`; scripts: `build:css`, `watch:css`.
- `web/static/css/input.css`: `@import "tailwindcss";` + `@theme` block mapping the design tokens from `design-project/colors_and_type.css` (bg, surface, fg, muted, border, accent family, freshness scale, danger, radius scale, shadows, fonts, `--shell-max`).
- One minimal Askama `layouts/base.html` (header placeholder, flash region placeholder, footer) + a simple error page template proving the render loop; styled with Tailwind utilities using the theme tokens.
- Compiled `app.css` is committed so `cargo run` works without Node; regeneration documented in README.

---

## 7. Test-support crate API (initial)

```rust
pub struct TestDb { pool: PgPool }          // migrates once (OnceLock), owns admin handle
impl TestDb {
    pub async fn begin() -> TestDb;         // BEGIN; returns handle for the test
}
pub struct TestTx<'a> { tx: Transaction<'a, Postgres> }
impl TestTx<'_> {
    pub async fn savepoint(&mut self) -> TestTx<'_>; // nested transaction = SAVEPOINT
    pub async fn commit(self);                       // optional inner commit
    // rollback happens on Drop
}
// Builders (grow every milestone):
pub struct UserBuilder { email: String, name: Option<String> }
impl UserBuilder { pub async fn create(&self, tx: &mut TestTx<'_>) -> Result<User>; }
```

Tests live in the owning crate (domain test in `domain`, DB integration test in `infrastructure`/`web`) and use `test-support` as a dev-dependency.

**M0 tests:**
1. *Domain:* a pure unit test (e.g. `UserId`/`UserEmail` value-object validation — non-empty, lowercase-normalized email shape) — no I/O.
2. *Integration (real Postgres):* `UserBuilder` inserts a user inside the test transaction; a hand-written SQLx query reads it back; assert fields; second insert with same email (different case) fails on the unique index; everything rolls back.
3. *HTTP:* `tower::ServiceExt::oneshot` tests for `/healthz` (200) and `/readyz` against a router wired to the real pool (200) and to a probe configured with a bogus URL (503).

---

## 8. Task breakdown (execution order)

1. **Tooling check:** `cargo --version`, `node --version`, docker up.
2. **Workspace scaffold:** root `Cargo.toml` (workspace + `[workspace.dependencies]`), `cargo new` the five crates; enforce dep direction by review (no sqlx/axum in domain/application).
3. **Docker:** `docker-compose.yml` (postgres:17, postgis image variant, healthcheck, named volume, port 5432, credentials via `.env`), `.env.example`.
4. **Migrations:** `migrations/0001_init.sql`; migration runner wired into app startup (`sqlx::migrate!`) and into `test-support`.
5. **Infrastructure:** config loader, `Db::connect`, `DatabaseProbe` impl, `Clock` impl (std/chrono).
6. **Domain:** `User` entity + `UserEmail` value object + domain error type; unit test.
7. **Application:** `CheckReadiness` use case + `DatabaseProbe` port.
8. **Web:** axum router, `AppState`, `/healthz`, `/readyz`, minimal Askama base layout + error page, static file serving, graceful startup logging.
9. **Frontend:** package.json + Tailwind entry CSS with `@theme` tokens; compile; link from base layout.
10. **Test-support:** fixture, transaction/SAVEPOINT helpers, `UserBuilder`.
11. **Tests:** integration (user insert/read/unique), HTTP tests for health/ready.
12. **Docs:** README (prereqs, `docker compose up`, `npm ci && npm run build:css`, `cargo run`, `cargo test`), verify fresh-clone flow.
13. **Verify:** run everything; `curl /healthz` 200; stop DB → `/readyz` 503; restart → 200; full `cargo test` green.

Dependencies added via `cargo add` only (§11), e.g.:
`cargo add axum tokio tower-http --features ...` (web), `cargo add sqlx --features runtime-tokio-rustls,postgres,migrate,chrono,uuid` (infrastructure), `cargo add chrono thiserror` etc. Exact feature sets verified at `cargo add` time.

---

## 9. Risks / open points

- sqlx compile-time checking (`query!`) requires `DATABASE_URL` or offline `sqlx-data` at build time; M0 uses runtime queries (`query` without macro) for the probe, macros start in M1 with `.env` documented — keeps onboarding friction low.
- PostGIS image choice: `postgis/postgis:17-3.5` has **no linux/arm64 manifest** (decided during implementation): the compose file uses `imresamu/postgis:17-3.5`, an arm64-compatible PostGIS build on PG17. Revisit for amd64-only production.
- Port conflicts on 5432: compose maps a non-default host port if needed; documented in README.
- Tailwind 4.3 exact npm version verified at install time; if 4.3 is unavailable on npm, pin the closest 4.x and note it here.

## 10. Definition of done checklist

- [ ] `docker compose up -d` starts Postgres+PostGIS healthy
- [ ] `cargo run` serves; `/healthz` 200; `/readyz` 200 with DB up, 503 with DB down, 500 semantics for app error distinct
- [ ] Migration 0001 applied automatically; `users` + PostGIS present
- [ ] `cargo test` green: domain test, DB integration test (rollback isolated), HTTP tests
- [ ] Tailwind pipeline compiles; base layout renders styled error page with design tokens
- [ ] README onboarding works from a fresh clone
- [ ] No Ledger entries created (none needed)
