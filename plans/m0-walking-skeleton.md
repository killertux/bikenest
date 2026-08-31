# M0 — Walking Skeleton

> Detailed plan for the first milestone. Companion to `PLAN.md` (overview + ledger) and `REQUIREMENTS.md` (rules). Slug: `walking-skeleton`.

## 1. Objective

Stand up a minimal but **real** vertical slice of the application: a Rust workspace following Clean Architecture, running against a real PostgreSQL + PostGIS instance, with the test loop (`cargo test`) proven end to end using the transaction-per-test strategy from `REQUIREMENTS.md` §113.

There is **no user-facing product feature** in M0. The deliverable is the skeleton that every later milestone builds on: a running server with health/readiness, a real database, reproducible migrations, and a test harness that makes `§113`-style tests natural.

## 2. Scope

### In scope

- Cargo workspace with the crate decomposition and dependency direction from `REQUIREMENTS.md` §4.
- `docker-compose.yml` providing PostgreSQL + PostGIS with a health check and a named volume.
- Migration tooling and the first migration (enable PostGIS; create a minimal `users` table).
- Configuration loading (`.env.example`, no committed secrets).
- `/healthz` and `/readyz` endpoints wired **through** the web → application → infrastructure layers, not a raw handler.
- One real domain entity (`User`), one real repository (`PgUserRepository`), one real use case (`CreateUser`), exercising real SQLx against real PostgreSQL.
- `test-support` crate: `TestDb` (transaction-per-test + rollback), a SAVEPOINT helper (§51), and a minimal builder.
- README onboarding: prerequisites, `docker compose up -d`, migrations, `cargo run`, `cargo test`.

### Out of scope (deferred)

- Any search/map/parking feature (M1).
- Authentication, sessions, roles (M2).
- Any external provider (geocoding, email, OAuth, image storage) — nothing to fake yet.
- Rate limiting, security headers, i18n, SEO (later milestones).
- Full schema (§8's complete table list) — only what M0 needs to prove the loop.

## 3. Success criteria (definition of done)

1. `docker compose up -d` brings up PostgreSQL + PostGIS with a green health check.
2. `cargo run -- migrate` (or the chosen command) runs migrations idempotently.
3. `cargo run` serves HTTP; `/healthz` returns `200`; `/readyz` returns `200` when the DB is up and `503` when it is not.
4. `cargo test` runs against real PostgreSQL, with each DB-backed test isolated by transaction + rollback.
5. The test suite includes at least:
   - one pure **domain** test (no DB);
   - one **integration** test that saves and reads a `User` through the real repository within a test transaction;
   - one **SAVEPOINT** test proving nested-transactional behavior (§51);
   - one **HTTP** test hitting `/healthz` through a real Axum app.
6. A fresh clone can reach all of the above by following the README alone.
7. Dependency direction is respected: `domain` depends on nothing external; `web`/`infrastructure`/`test-support` point inward.

## 4. Architecture decisions made in M0

These are decisions the implementer must fix and document (per `REQUIREMENTS.md` §112.1–2).

### D1 — Crate decomposition

```text
crates/
├── domain/           # entities, value objects, domain errors  (no external deps)
├── application/      # use cases + port traits                  (depends on domain)
├── infrastructure/   # SQLx persistence, config, clock, health  (depends on application + domain)
├── web/              # Axum app + main.rs (composition root)    (depends on application + infrastructure)
└── test-support/     # TestDb, SAVEPOINT helper, builders       (depends on domain, application, infrastructure)
```

**Dependency graph (must hold):**

```text
            web ───────────────┐
              │                │
              ▼                ▼
        application ◄──── infrastructure
              │                  │
              ▼                  ▼
           domain ◄──────────────┘
              ▲
              │
        test-support
```

- `domain` MUST NOT depend on Axum, SQLx, Askama, HTMX, PostgreSQL, or any external client (§4).
- `application` defines ports (traits); `infrastructure` implements them.
- `web` holds `main.rs` and is the **composition root** (it constructs real infra impls and injects them). This is the one deliberate exception to "web only does HTTP" — `main.rs` wiring is a thin composition boundary, not business logic.
- `test-support` is a normal crate used by tests across the workspace.

### D2 — Connection strategy (the heart of §113)

- Repositories are written against **`&mut PgConnection`** (or `impl Executor`), **not** `&PgPool`.
- The web layer acquires a connection from the pool per request and hands `&mut conn` to the use case.
- Tests acquire a single `PgConnection`, begin a transaction, and hand `&mut tx` to the **same** repository code — so production and test run the identical code path, and rollback undoes everything.

This is what makes the §113 test shape (begin → arrange → invoke → assert → rollback) natural. The alternative (passing `&PgPool` into repositories) makes per-test transactions impractical, so we reject it in M0.

### D3 — Transaction/SAVEPOINT mechanism (§51)

- Use cases that need atomicity must not open a second independent `BEGIN` on a connection already in a test transaction.
- Introduce a minimal **unit-of-work** abstraction: a `Transaction`/`UnitOfWork` port in `application` that wraps "run this closure transactionally."
- Two impls: `PgUnitOfWork` (production: `pool.begin()`) and, in `test-support`, a **SAVEPOINT**-based impl over the test transaction (`SAVEPOINT name` … `RELEASE`/`ROLLBACK TO`).
- The exact SQLx call is `sqlx::raw_sql` / `sqlx::query` issuing `SAVEPOINT` statements on the connection; `test-support` exposes an ergonomic wrapper so test authors never hand-write it.

### D4 — Async port traits

- Use **`async-trait`** for async port traits and inject them as `Arc<dyn Port>`. This is the conventional, low-friction choice for dyn-dispatchable ports (fakes land behind the same traits in later milestones).
- Fallback if the team prefers zero-dyn: make use cases generic over the port. Not required for M0; note it and move on.

### D5 — SQLx compile-time checking

- Use `sqlx::query!` (compile-time checked) where practical (§9).
- Commit a **`.sqlx` offline cache** generated via `cargo sqlx prepare` so builds/CI don't need a live database at compile time.
- Dev workflow: `sqlx migrate run` against the Docker DB, then `cargo sqlx prepare` to refresh the cache after schema changes.

### D6 — Rust edition / MSRV

- Rust **edition 2024** on the latest stable toolchain. (No nightly.)

## 5. Deliverables (file/dir tree)

```text
bikenest/
├── Cargo.toml                    # [workspace]
├── .env.example
├── .gitignore                    # + .env, target/, .sqlx? (see note)
├── docker-compose.yml
├── README.md                     # onboarding (updated)
├── migrations/
│   └── 0001_init.sql
├── crates/
│   ├── domain/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── user.rs           # User entity
│   │       └── error.rs          # DomainError
│   ├── application/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── ports/
│   │       │   ├── mod.rs
│   │       │   ├── user_repository.rs   # UserRepository trait
│   │       │   ├── health.rs            # HealthCheck trait
│   │       │   └── unit_of_work.rs      # UnitOfWork trait (D3)
│   │       └── use_cases/
│   │           ├── mod.rs
│   │           └── create_user.rs        # CreateUser use case
│   ├── infrastructure/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── config.rs                 # AppConfig + env loading
│   │       ├── db.rs                     # pool + migration runner
│   │       ├── health.rs                 # PgHealthCheck
│   │       ├── unit_of_work.rs           # PgUnitOfWork
│   │       ├── user_repository.rs        # PgUserRepository
│   │       └── clock.rs                  # SystemClock
│   ├── web/
│   │   └── src/
│   │       ├── lib.rs                    # build_router + AppState
│   │       ├── main.rs                   # composition root
│   │       ├── state.rs                  # AppState
│   │       └── routes/
│   │           ├── mod.rs
│   │           └── health.rs             # /healthz, /readyz
│   └── test-support/
│       └── src/
│           ├── lib.rs
│           ├── db.rs                     # TestDb (transaction + rollback)
│           ├── savepoint.rs              # SAVEPOINT helper (D3)
│           └── builders.rs               # TestUser builder (minimal)
```

## 6. Dependencies

Scaffold with `cargo new --lib` per crate, then add external deps with `cargo add` (per §11 — do not hand-pin versions). Internal deps are path-based.

```bash
# domain
cargo add -p domain thiserror uuid --features uuid/serde,uuid/v4 chrono --features chrono/serde

# application (depends on domain via path)
cargo add -p application async-trait thiserror uuid chrono

# infrastructure (depends on application + domain via path)
cargo add -p infrastructure \
  sqlx --features runtime-tokio-rustls,postgres,uuid,chrono,migrate \
  tokio --features macros,rt-multi-thread \
  thiserror dotenvy serde --features serde/derive serde_json tracing

# web (depends on application + infrastructure via path)
cargo add -p web \
  axum tokio --features macros,rt-multi-thread,net,signal \
  tower-http --features trace \
  tracing tracing-subscriber --features tracing-subscriber/env-filter \
  dotenvy serde --features serde/derive

# test-support (depends on domain, application, infrastructure via path)
cargo add -p test-support sqlx --features runtime-tokio-rustls,postgres,uuid,chrono,migrate tokio --features macros uuid chrono thiserror
```

Path dependencies are added by editing each crate's `Cargo.toml` (workspace-internal, no version needed), e.g. `domain = { path = "../domain" }`.

> The implementer MUST verify current feature flags with `cargo add` rather than copying the above verbatim (§11).

Install tooling once: `cargo install sqlx-cli --no-default-features --features rustls,postgres`.

## 7. Migrations

- Tooling: **sqlx migrations** (`sqlx-cli`), directory `migrations/`.
- The app also exposes a migration command (`cargo run -- migrate`) that calls `sqlx::migrate!` at runtime, so "run migrations" is one project command (§90).

`migrations/0001_init.sql` (minimal — this is the real `users` table M2 will extend):

```sql
CREATE EXTENSION IF NOT EXISTS postgis;

CREATE TABLE users (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    email       text NOT NULL UNIQUE,
    created_at  timestamptz NOT NULL DEFAULT now()
);
```

(Requires `pgcrypto` for `gen_random_uuid()`, or generate UUIDs in Rust — see Open questions. Prefer `CREATE EXTENSION IF NOT EXISTS pgcrypto;`.)

## 8. Configuration

`.env.example`:

```text
DATABASE_URL=postgres://bikenest:bikenest@localhost:5432/bikenest
APP_PORT=3000
RUST_LOG=info
```

- Loaded via `dotenvy` into an `AppConfig` struct; secrets are never committed (`.gitignore` includes `.env`).
- No secret-bearing external providers yet, so defaults are sufficient.

## 9. Docker Compose

```yaml
services:
  postgres:
    image: postgis/postgis:16-3.4
    environment:
      POSTGRES_USER: bikenest
      POSTGRES_PASSWORD: bikenest
      POSTGRES_DB: bikenest
    ports:
      - "5432:5432"
    volumes:
      - pgdata:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U bikenest -d bikenest"]
      interval: 5s
      timeout: 3s
      retries: 10

volumes:
  pgdata:
```

- Image tag pinned and verified at implementation time (PostGIS must be enabled — §89).
- Named volume preserves local data across restarts (§94); `docker compose down -v` resets.

## 10. Health & readiness

- `GET /healthz` → `200` always (process alive).
- `GET /readyz` → pings the DB (`SELECT 1` through the pool); `200` if reachable, `503` if not. This distinguishes "DB down" from "app error" (§87).
- Both go **through** `web` handler → `application::ports::HealthCheck` → `infrastructure::PgHealthCheck`, so the layer boundary is exercised, not bypassed.

## 11. Test infrastructure

`test-support` provides:

- **`TestDb`** — connects to the test database (same Postgres/PostGIS as prod, §49), runs pending migrations once (or uses a migrated template DB), begins a transaction, and rolls back on `Drop` regardless of pass/fail (§50).
- **`savepoint()`** — wraps a closure in `SAVEPOINT`/`RELEASE` (or `ROLLBACK TO` on error) over the test transaction (§51).
- **`TestUser::new(&mut conn).with_email(…).create().await?`** — a minimal builder demonstrating the §53–54 pattern, expanded in later milestones.

Test database strategy (§93): use a dedicated database (e.g. `bikenest_test`) reachable via `TEST_DATABASE_URL`, auto-created/migrated by `TestDb` so `cargo test` needs no manual setup. Document how the connection is obtained in the README.

## 12. Task breakdown (ordered)

1. **Workspace scaffold** — `Cargo.toml` `[workspace]` + the five crates via `cargo new --lib`.
2. **Dependencies** — add the §6 deps and path deps; ensure `cargo build` succeeds on an empty workspace.
3. **Docker + env** — `docker-compose.yml`, `.env.example`, `.gitignore`.
4. **Migrations** — `migrations/0001_init.sql`; wire `sqlx::migrate!` into `infrastructure::db`; `cargo run -- migrate` works.
5. **`domain`** — `User` (id, email, created_at) + `DomainError` + a pure unit test (invalid email is rejected).
6. **`application`** — `UserRepository`, `HealthCheck`, `UnitOfWork` ports; `CreateUser` use case (validates email, saves via port).
7. **`infrastructure`** — `AppConfig`, pool + migration runner, `PgHealthCheck`, `PgUserRepository` (real `query!`), `PgUnitOfWork`, `SystemClock`.
8. **`web`** — `AppState`, `routes::health`, `build_router`, `main.rs` composition root wiring real impls.
9. **`test-support`** — `TestDb` (transaction + rollback), `savepoint()` helper, `TestUser` builder.
10. **Tests** — the five tests in §3.5, all green against the Docker DB.
11. **README** — onboarding per §92 (prereqs, setup, env vars, DB init, migrations, run, test, reset).
12. **Verification pass** — run §13 checklist; confirm dependency direction (e.g. `cargo tree` shows `domain` with no external deps).

## 13. Verification

Manual (fresh-clone simulation):

```bash
git clone <repo> && cd bikenest
docker compose up -d
cargo run -- migrate
cargo run              # serves on :3000
curl localhost:3000/healthz   # 200
curl localhost:3000/readyz    # 200 with DB up; stop postgres -> 503
cargo test                    # all green, rollback-isolated
```

Automated: the five tests from §3.5.

## 14. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Transaction-per-test with a pool (connection checkout) is the classic SQLx footgun | D2: repositories take `&mut PgConnection`; tests use a single connection + transaction |
| Nested `BEGIN` in tests | D3: `UnitOfWork` + SAVEPOINT in `test-support` (§51) |
| `query!` needs a DB (or offline cache) at build time | D5: commit `.sqlx` offline cache via `cargo sqlx prepare` |
| PostGIS image/version drift | Pin the `postgis/postgis` tag; verify `PostGIS_Version()` in a smoke test |
| Over-engineering the skeleton | Only build what §3 needs; defer fakes, security headers, i18n |
| `async-trait` vs native async-fn-in-trait churn | Fix D4 in M0 and don't revisit until there's a concrete reason |

## 15. Ledger additions

**None.** M0 introduces no mock data or fakes (the test database is real, not a substitute). If a decision later requires temporary scaffolding, it gets a Ledger entry at that time.

## 16. Open questions

1. **UUID generation** — `gen_random_uuid()` (pgcrypto extension) vs Rust-side `uuid::Uuid::new_v4()`? (Recommend Rust-side to keep DB migrations simpler; either is fine.)
2. **`async-trait` vs native async traits** — D4 recommends `async-trait`; confirm before locking in.
3. **Test database naming/provisioning** — `bikenest_test` auto-created by `TestDb`, or a `docker-compose` second service? (Recommend `TestDb` self-provisions against the same container.)
4. **`.sqlx` in `.gitignore`** — must be **committed** (not ignored) for offline CI builds; confirm.
5. **Binary location** — `web/src/main.rs` (chosen) vs a separate thin `server` crate; confirm we stay in `web` for now.
