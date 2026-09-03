# Testing

How BikeNest is tested and how to write a test.

## Running tests

```bash
cargo test                    # domain + application only (no database needed)
docker compose up -d db       # required once, before DB-backed tests
cargo test --workspace        # everything, including #[db_test] integration/HTTP tests
cargo test -p bikenest-web    # a single crate
cargo test search_            # filter by name substring
```

`cargo build` itself needs **no database** (queries are runtime-checked), but
the DB-backed tests do — they connect to the compose database
(`TEST_DATABASE_URL`, falling back to `DATABASE_URL`).

## The four layers

| Layer | Where | Kind | Needs DB? |
|---|---|---|---|
| Domain | `crates/domain/src/*.rs`, inline `#[cfg(test)] mod tests` | pure unit tests (hours/DST, freshness, cost, security, scoring, confidence rules) | no |
| Application | `crates/application/tests/*.rs` | use-case tests with in-memory/fake ports | no |
| Infrastructure | `crates/infrastructure/tests/*.rs` | real-Postgres integration tests against `Sqlx*` repositories | yes |
| Web | `crates/web/tests/*.rs` | HTTP tests through the real router | yes |

## The `#[db_test]` harness

`#[db_test]` (from `bikenest_test_support`) turns an `async fn` into a test that
runs against **real PostgreSQL** inside a transaction that is **automatically
rolled back** at the end. All `#[db_test]`s share one multi-threaded tokio
runtime, one connection pool, and one migration pass — so the suite is fast and
tests are isolated with zero cleanup code.

```rust
use bikenest_test_support::db_test;

#[db_test]
async fn my_test(tx: &mut TestTx) {
    // tx.executor() runs SQL inside the test's transaction.
    sqlx::query("INSERT INTO ...").execute(tx.executor()).await.unwrap();
    // ...assert...
    // tx drops here → rollback; nothing persists.
}
```

Rules:

- The function must take exactly one parameter: `tx: &mut TestTx`.
- Use `tx.executor()` for any SQL that must be visible to code running inside
  the same transaction.
- **Never** `block_on` inside a `#[db_test]` body — await `pool()` instead.
- An open **savepoint** simulates an inner application transaction committing:
  `let mut sp = tx.savepoint().await;` … `sp.commit().await;` (or
  `sp.rollback().await`).

### When rollback isn't enough: the committed-fixture pattern

Read-model tests query through *other* pool connections, which cannot see the
uncommitted rows of the test transaction. For those, commit a **tagged**
fixture, assert against the real readers, then delete by tag. See
`crates/infrastructure/tests/parking_test.rs` for the canonical example:

```rust
const MARK: &str = "fix-within-radius";

#[db_test]
async fn within_radius_ordered_by_distance(tx: &mut TestTx) {
    cleanup_fixture(MARK).await;                       // delete leftover rows by seed_key
    ParkingBuilder::new()
        .with_fixture_tag(MARK)
        .at(lat, lon)
        .create(tx.executor()).await.unwrap();
    tx.commit_fixture().await;                         // commit, then start a fresh tx

    let page = real_search(&request).await.unwrap();   // reads via the pool
    assert!(/* ... */);

    cleanup_fixture(MARK).await;                       // leave no trace
}
```

`ParkingBuilder::with_fixture_tag(marker)` writes the marker into the
`seed_key` column; `cleanup_fixture` deletes by it. Give each test a unique tag
and a geographically separated origin so crashed runs can't cross-contaminate.

## Builders and doubles

`bikenest_test_support` provides domain-rich builders and fast fakes:

- **`UserBuilder`** — creates `users` rows and returns a domain `User`.
- **`ParkingBuilder`** — creates a full `parking_location` (hours, security
  tri-state, rating, moderation state, fixture tag, version). Chainable:
  `.with_cost(...)`, `.at(lat, lon)`, `.with_hours(1..=5, (6,0), (20,0))`,
  `.with_security("cctv", 1)`, `.verified_days_ago(3)`, …
- **`TestPasswordHasher`** — a non-cryptographic hash (prefix `test:`) so the
  web/HTTP suite never pays for argon2.
- **`TestObjectStorage`** — in-memory object storage double.
- **`pool()`** — the shared, migrated connection pool, for wiring routers.

For HTTP tests, inject doubles through the test constructor:

```rust
let db = Db::from_pool(pool().await);
let app = bikenest_web::app_router_with(
    db,
    std::time::Duration::from_secs(2),
    Box::new(FakeEmailProvider::default()),       // or a capture double
    bikenest_infrastructure::FakeOAuthProvider::default(),
    TestPasswordHasher,
    Box::new(bikenest_infrastructure::InMemoryRateLimiter::default()),
    std::sync::Arc::new(TestObjectStorage::default()),
);
let res = app.oneshot(Request::builder().uri("/").body(Body::empty()).unwrap()).await.unwrap();
```

## Writing a new test, step by step

### 1. Domain rule (no DB)

Put it inline next to the code, or in the crate's tests. Pure `assert_eq!` on
domain values.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paid_cost_with_price_roundtrips() {
        let money = Money::new(1250, CurrencyCode::BRL, PricingUnit::PerDay).unwrap();
        match Cost::paid(money) {
            Cost::Paid { price: Some(p) } => assert_eq!(p.cents(), 1250),
            _ => panic!("expected paid"),
        }
    }
}
```

### 2. Application use case (in-memory ports, no DB)

Construct the service with hand-rolled or in-memory port impls and assert on
the returned domain result. Look at `crates/application/tests/*.rs` for existing
double patterns.

### 3. Repository / persistence (DB-backed)

Use `#[db_test]` + builders + `tx.executor()`:

```rust
#[db_test]
async fn user_builder_persists_a_user(tx: &mut TestTx) {
    let user = UserBuilder::new()
        .with_email("ada@example.com")
        .create(tx.executor()).await.unwrap();
    assert_eq!(user.email().to_string(), "ada@example.com");
    // rollback happens automatically
}
```

### 4. HTTP endpoint (DB-backed, through the real router)

Use `#[db_test]` + `pool()` + `app_router(_with)` + `tower::ServiceExt::oneshot`:

```rust
#[db_test]
async fn healthz_is_alive(_tx: &mut TestTx) {
    let app = bikenest_web::app_router(Db::from_pool(pool().await), std::time::Duration::from_secs(2));
    let res = app.oneshot(
        Request::builder().uri("/healthz").body(axum::body::Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}
```

Pin the locale for deterministic copy with `Accept-Language: en` (the default
falls back to pt-BR).

## Naming and behavior

- Test names describe the *behavior*, not the method: `within_radius_ordered_by_distance`,
  `readyz_returns_ready_with_real_database`, `security_headers_present_on_public_page`.
- One assertion or one coherent scenario per test; prefer many small tests over
  one large one.
- Prefer real infrastructure over mocks at the persistence layer (`#[db_test]`
  against Postgres) and fakes only at the *external-provider* boundary (email,
  OAuth, storage, geocoding, rate limiter).
- Keep argon2 and ValKey out of the suite with `TestPasswordHasher` and the
  in-memory limiter, so tests stay fast and deterministic.
