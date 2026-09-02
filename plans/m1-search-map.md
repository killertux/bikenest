# M1 — Core search & map (read-only) — implementation plan

> **Status: implemented (all tasks complete), plus post-M1 additions — see the addendum below.**
> Tests: 62 green (domain 18, application 13, infrastructure 22, web 9). Deviation from the plan's
> test-strategy row: read-model tests whose queries run on other pool connections cannot see the test
> transaction's uncommitted rows, so they use a **committed-fixture pattern** (rows tagged with a
> unique `seed_key` fixture marker, committed via `TestTx::commit_fixture()`, asserted against the
> real readers, then deleted by tag). Transaction-per-test rollback still governs all other tests.
> MapLibre pinned to 4.7.1 (v6 dropped the UMD script build; revisit with a bundler in M7).

## Addendum — additions beyond the original M1 scope

Work delivered on top of the M1 plan above, some of it pulled forward from later milestones. The
milestone plan (`PLAN.md`) and its Ledger were updated to match.

| Area | What changed | Origin | Notes |
|---|---|---|---|
| **Geography** | Dataset + `FakeGeocoder` relocated from São Paulo to **Curitiba** (24 locations; tz stays `America/Sao_Paulo`). | M1 | `devdata.rs`, `geocoding.rs` centroid `(-25.4284, -49.2733)`. |
| **Object storage** | `ObjectStorage` port (`application`) + `LocalDiskStorage` (`infrastructure`) issuing **HMAC-signed, expiring `/media/{key}` GET URLs** (S3-presign parity). Served via a `GET /media/{*key}` route that verifies signature + expiry. | pulled from **M4** | Ledger #7 now "M1". `hmac`+`sha2` added via `cargo add`. Config: `MEDIA_ROOT`, `MEDIA_SIGNING_SECRET` (Ledger #14). |
| **Photos** | `parking_photo` table (migration `0003`); `ParkingPhotoReader` port; seeder stores real images through the port; P3 **gallery + lightbox**; primary photo on P2/P1 cards. Photos default `APPROVED` for the demo. | pulled from **M4** | The full upload/validation/EXIF/thumbnail/moderation pipeline and the `PENDING_REVIEW` default remain M4. |
| **i18n** | Bilingual catalog (`crates/web/src/i18n.rs`), `Translator` threaded through all pages + labels, locale resolved from `Accept-Language` (fallback pt-BR) and overridable via a `lang` cookie set by `GET /lang/{pt-br\|en}`. | pulled from **M7** (§102) | Ledger #11 largely resolved. HTTP tests pin `Accept-Language: en`. |
| **Security labels** | `security_feature` **DB catalog table dropped** (migration `0004`, incl. the `parking_security` FK). Canonical codes are now `bikenest_domain::SECURITY_FEATURE_CODES`; labels are i18n (`security.<code>`). | M1 (design change) | Supersedes the §3 schema (which created `security_feature`). Labels are now localizable; adding an attribute is a code + translation, no migration. |
| **Frontend** | `hx-boost="true"` on `<body>` + vendored `hx-alpine-compat` extension (htmx → extension → Alpine order); `search.js` made idempotent/boost-safe; lang toggle returns via `HX-Current-URL`/`Referer`. | M1 | Interacts with a future strict CSP — tracked as Ledger #15. |
| **Docker** | Fixed the compose `css` service (glibc image + TTY so Tailwind `--watch` survives) and the `app` service (a YAML `>` newline made it run `cargo check`, never the server). | M1 (dev-env fix) | See `docker-compose.yml` comments. |
| **Tests / robustness** | +8 domain unit tests (cost/type/currency §9), +4 `LocalDiskStorage` tests; the P3 details HTTP test is now self-contained (own committed fixture, no `seed-mock` dependency). | M1 | 50 → 62 green. |

Companion to `REQUIREMENTS.md` (§20–§34 drive this milestone), `PLAN.md` (M1 overview) and
`UI_DESIGN.md` + `design-project/` screens `p1-landing.html`, `p2-search.html`,
`p3-parking-details.html`, `p7-about.html` (visual contract).

**Goal:** the read-only product loop over mock data — type a destination, see nearby parking on a
map and as an accessible list, filter/sort, open a details page. No authentication anywhere.

**Working app means (acceptance):** open `/`, type "Rua XV de Novembro" → coordinates resolve
(fake geocoder) → mock parking appears on map **and** in the list → filter by cost/type/security →
change sort → paginate → open a details page. The list is fully usable with JavaScript disabled.
`cargo test` green; fresh clone onboarding from README still works.

---

## 1. Scope

### In scope

| Area | Content |
|---|---|
| Schema | `0002_parking.sql`: `parking_location` (+ PostGIS point + GiST index), `security_feature` catalog, `parking_security` tri-state join, `opening_hours` wall-clock rows |
| Domain | `ParkingLocation` aggregate + value objects: `ParkingType`, `Cost`/`Money`, `SecurityAttribute` (tri-state), `OpeningHours` (open-now computation), `Freshness` (configurable thresholds), `GeoPoint`, `ModerationState` (full enum, only `ACTIVE` used in M1) |
| Application | `Geocoder` port; `FakeGeocoder` wiring behind port (impl in infrastructure); `SearchParking` + `GetParkingDetails` use cases; recommendation scoring (pure, unit-tested); keyset cursor codec |
| Infrastructure | `SqlxParkingSearchReader` / `SqlxParkingDetailsReader` with **compile-time checked `query!` macros** (starts here per plan); `FakeGeocoder` implementation |
| Seed data | `seed-mock` subcommand: deterministic ~24 locations around Curitiba landmarks (**Ledger #1**), idempotent via nullable `seed_key` column |
| Web | Routes `GET /` (P1), `GET /search` (P2), `GET /parking/{id}` (P3), `GET /about` (P7); HTMX results fragment; Alpine map/marker sync; vendored assets |
| Frontend | Askama pages/components/partials per design screens; vendored maplibre-gl + alpinejs + htmx; `map.css` for MapLibre overrides |

### Explicitly out of scope (deferred, with where it lands)

| Item | Lands in |
|---|---|
| Reviews & rating aggregation (tables, forms) | M3 (`rating_avg`/`rating_count` columns exist in M1; only the seeder fills them) |
| Photos (tables, gallery content) | M4 (details page shows an empty-state block) |
| Auth-gated actions on P3 (favorite, report, propose change, verify) | M3/M5 (hidden entirely in M1 — no dead buttons) |
| Search rate limiting (§21) | M3, with the in-memory limiter (Ledger #6) |
| Real geocoder / real tiles | M7 (Ledger #2/#3) |
| Browser geolocation persistence | never by default (§22 — location is used and discarded) |

---

## 2. Decisions

| Decision | Choice | Reasoning |
|---|---|---|
| PostGIS column type | `location geography(Point, 4326)` with GiST index; `lat`/`lon` exposed as `GENERATED ALWAYS AS (ST_Y(location::geometry)) STORED` / `ST_X` columns | Single source of truth for coordinates; generated columns are immutable functions, so this is valid Postgres |
| Distance semantics | `ST_DWithin(location, $point::geography, $radius_m)` / `ST_Distance(... ::geography)` | Geography gives true meters on the spheroid; radius filters 250/500/1000/2000 m per §31 |
| Parking type storage | `parking_type TEXT NOT NULL` (no Postgres enum), validated in domain | §26 requires extensibility without migrations |
| Cost storage | `cost_kind TEXT` ∈ {free, paid, unknown} + nullable `price_cents BIGINT`, `price_currency CHAR(3)`, `price_unit TEXT` | §27: Free/Paid/Unknown is distinct from "price not currently known" — that nuance lives in domain (`Cost::Paid { amount: Option<Money> }`); cents avoid float money |
| Security tri-state | `parking_security(location_id, feature_code, state SMALLINT)` with state ∈ {0=unknown, 1=yes, 2=no}; feature catalog `security_feature(code PK, label)` | §28: unknown ≠ false; catalog rows inserted by migration |
| Opening hours | `opening_hours(location_id, day_of_week SMALLINT (ISO 1=Mon..7=Sun), opens_at TIME, closes_at TIME, all_day BOOLEAN)` — multiple rows per day allowed; `hours_unknown BOOLEAN` on `parking_location` | §29: wall-clock times + location IANA tz, never UTC; three states: unknown (`hours_unknown=true`), closed all week (no rows, flag false), schedule (rows). 24h = `all_day` row |
| Location timezone | `timezone TEXT NOT NULL` (IANA), derived by the seeder from coordinates via `chrono-tz`-assisted lookup table (mock dataset); a coordinate→IANA resolver becomes a port in M3 when contributors add locations | §29; M1 has no user input, so a static mapping for the mock city is enough |
| Open-now | Computed **in SQL** inside the search query: `(statement_local_timestamp AT TIME ZONE pl.timezone)` → wall clock → join `opening_hours` on `EXTRACT(ISODOW ...)`, honoring `all_day` | Keeps pagination honest (filter applies before keyset slicing); DST corner cases documented as acceptable for M1 (the dataset's `America/Sao_Paulo` tz has had no DST since 2019) |
| Pagination | Keyset `(order_value, id)` cursor, opaque base64-JSON; page size default 20, max 100 (§32) | No offsets; deterministic under inserts within a page |
| Recommended sort | Scored **in Rust** (application layer, unit-tested); fetch radius-capped candidate set (hard cap 500) and paginate in memory for this sort only; other sorts keyset in SQL | §34 wants weights configurable in code and the algorithm deterministic + unit-testable; a giant scoring SQL expression would be untestable. 500-cap documented; revisited in M7 perf validation |
| Scoring neutrality | Missing inputs → neutral 0.5, never worst (§34); tie-break by (score DESC, id ASC) | §34 explicit requirement |
| Freshness thresholds | `FreshnessConfig` constants in application config (30/90/180/365 days — §40 defaults), injected into use cases | Ledger #9-style configurability from day one (the *parking-side* thresholds; the review-side ones arrive M3) |
| Compile-time SQL | `sqlx::query!` macros from M1 onward; requires `DATABASE_URL` at compile time (sqlx reads the repo-root `.env` automatically) | §9 "SHOULD use compile-time checking where practical"; M1 is the first real query milestone. No offline cache yet (M7); README documents "compose up before building" |
| Cursor codec | `base64(json({"v": <order value>, "id": <bigint>}))` in application layer | Opaque, URL-safe, tamper-evident enough for read-only use |
| Frontend assets | `maplibre-gl`, `alpinejs`, `htmx` installed via npm and **copied into `web/static/vendor/`** by `npm run build:assets`; committed to git | §12 spirit: no runtime CDN; `cargo run` keeps working without Node |
| HTMX version | `htmx.org@4.0.0` (published on npm under the `next` dist-tag; docs: four.htmx.org). Vendor the dist bundle like the other assets. htmx 4 specifics we rely on: `HX-Request` header still sent on every htmx request (plus new `HX-Request-Type: full\|partial`); **4xx/5xx responses now swap by default** (error pages must be designed as swap content — relevant to §116.6); attribute inheritance is explicit (`:inherited` suffix) — avoid relying on implicit inheritance in templates; `hx-disable` means "disable element during request" — use `hx-ignore` to exclude elements from processing | Matches §13's required version exactly; asset vendoring keeps the no-CDN rule |
| Map tiles | MapLibre style: `https://demotiles.maplibre.org/style.json` (MapLibre demo tiles), provider behind one JS constant + noted in **Ledger #3** | §23 provider replaceable; never production tiles |
| Map/list sync | One small `search.js`: Alpine store holds results + `selectedId`; HTMX result fragments embed results JSON in a `<script type="application/json" id="search-data">` block; markers rendered from store; card hover/click ↔ marker click | §14: Alpine for local UI state only; server stays source of truth |
| No-JS behavior | `/search` is a plain GET form; fragments are an enhancement (`HX-Request` header detection); list is the canonical non-map view (§23) | Accessibility requirement, cheap to do when planned from the start |
| Seeder mechanics | Nullable `seed_key TEXT` column on `parking_location`; `seed-mock` deletes `WHERE seed_key IS NOT NULL` then inserts its dataset in one transaction, recomputing rating/verification fields deterministically | Idempotent re-runs; dev-only rows are trivially identifiable for cleanup |
| FakeGeocoder determinism | Exact-match table for ~20 Curitiba landmarks (shared with the seeder's geography) + fallback: FNV-1a hash of normalized query → deterministic offset jitter (±2.5 km) around the Curitiba centroid; always returns a hit unless query is empty → `NotFound` for empty/whitespace only | Search demo always works for arbitrary input; behavior reproducible in tests |

---

## 3. Schema — `migrations/0002_parking.sql`

```sql
-- catalogs
CREATE TABLE security_feature (
    code   TEXT PRIMARY KEY,          -- 'dedicated_locking_point', 'indoor', ...
    label  TEXT NOT NULL
);
-- 8 rows from §28, seeded here.

CREATE TABLE parking_location (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name          TEXT NOT NULL,
    address       TEXT NOT NULL,
    description   TEXT,
    parking_type  TEXT NOT NULL,                  -- domain-validated enum (§26)
    cost_kind     TEXT NOT NULL,                  -- 'free' | 'paid' | 'unknown'
    price_cents   BIGINT,
    price_currency CHAR(3),
    price_unit    TEXT,                           -- 'hour'|'day'|'month'|'entry'
    location      geography(Point, 4326) NOT NULL,
    lat           double precision GENERATED ALWAYS AS (ST_Y(location::geometry)) STORED,
    lon           double precision GENERATED ALWAYS AS (ST_X(location::geometry)) STORED,
    timezone      TEXT NOT NULL,                  -- IANA (§29)
    hours_unknown BOOLEAN NOT NULL DEFAULT false,
    rating_avg    NUMERIC(3,2),                   -- denormalized aggregates;
    rating_count  INTEGER NOT NULL DEFAULT 0,     -- maintained by review use cases in M3
    moderation_state TEXT NOT NULL DEFAULT 'ACTIVE',  -- §25 enum (only ACTIVE in M1)
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_meaningful_update_at TIMESTAMPTZ,
    last_verified_at     TIMESTAMPTZ,
    seed_key             TEXT                      -- dev-only affordance (Ledger #1)
);
CREATE INDEX parking_location_location_gist ON parking_location USING GIST (location);
CREATE INDEX parking_location_state_idx ON parking_location (moderation_state);

CREATE TABLE parking_security (
    location_id  BIGINT NOT NULL REFERENCES parking_location(id) ON DELETE CASCADE,
    feature_code TEXT NOT NULL REFERENCES security_feature(code),
    state        SMALLINT NOT NULL,               -- 0 unknown | 1 yes | 2 no
    PRIMARY KEY (location_id, feature_code)
);

CREATE TABLE opening_hours (
    location_id  BIGINT NOT NULL REFERENCES parking_location(id) ON DELETE CASCADE,
    day_of_week  SMALLINT NOT NULL,               -- 1=Mon .. 7=Sun (ISO)
    opens_at     TIME NOT NULL,
    closes_at    TIME NOT NULL,
    all_day      BOOLEAN NOT NULL DEFAULT false,
    PRIMARY KEY (location_id, day_of_week, opens_at, closes_at)
);
```

State machine note (§25, defined now, enforced from M3 when writes exist):
`ACTIVE → FLAGGED → (ACTIVE | INVALID)`; `ACTIVE → INVALID → REMOVED`; `PENDING_REVIEW` only for
user-created locations (M3+). Public search filters `moderation_state = 'ACTIVE'` everywhere.

---

## 4. Domain model (crates/domain)

```
ParkingLocation { id, name, address, description, ParkingType, Cost, GeoPoint,
                  timezone: Tz (chrono-tz), OpeningHours, SecurityFeatures,
                  ModerationState, Rating { avg: Option<Decimal-ish f64-free…>, count },
                  freshness inputs (last_verified_at, last_meaningful_update_at), created_at }
```

- `ParkingType`: enum { Rack, ParkingFacility, Indoor, Secured, Locker, Other } — `as_code`/`from_code` (extensible: unknown code → error, explicit, not silently Other).
- `Cost`: enum { Free, Paid { price: Option<Money> }, Unknown } — `Money { cents: i64, currency: CurrencyCode, unit: PricingUnit }`.
- `SecurityState`: enum { Unknown, Yes, No } (tri-state, §28).
- `OpeningHours`: `Vec<DayHours>` + `is_unknown()`; `is_open_at(instant: DateTime<Utc>) -> bool` converts to location tz (chrono-tz) and compares wall-clock ranges; unit tests cover multiple periods/day, all-day rows, closed days, and a DST-crossing timezone (Europe/Berlin) to pin §29 behavior.
- `Freshness` (in application? — decision: pure function in domain over a `FreshnessThresholds` value passed in, so thresholds stay configurable): `from_last_verified(Option<DateTime<Utc>>, now, thresholds) -> FreshnessCategory` with categories Fresh/RecentlyVerified/Aging/Stale/VeryStale; `None` → `Unknown`.
- `GeoPoint { lat, lon }` with validation.
- `ModerationState`: full §25 enum; only `Active` is publicly searchable.

New domain deps: `chrono`, `chrono-tz` (both `cargo add`).

## 5. Application layer (crates/application)

Ports:

```rust
#[async_trait] trait Geocoder { async fn geocode(&self, q: &str) -> Result<Option<GeoHit>, GeocodeError>; }
#[async_trait] trait ParkingSearchReader {
    async fn search(&self, criteria: &SearchCriteria) -> Result<Vec<ParkingSummary>, ReadError>;
    async fn count(&self, criteria: &SearchCriteria) -> Result<i64, ReadError>;
}
#[async_trait] trait ParkingDetailsReader { async fn details(&self, id: i64) -> Result<Option<ParkingLocation>, ReadError>; }
```

Use cases:

- `SearchParking { geocoder, reader, scoring: RecommendationConfig, freshness: FreshnessConfig }`
  — flow: resolve origin (explicit lat/lon beats q; q → geocoder; neither → error `MissingDestination`);
  clamp radius to allowlist; build `SearchCriteria`; if sort == Recommended → fetch candidates (cap 500),
  score + sort + paginate in memory; else pass through and slice by cursor. Returns
  `SearchResult { items, next_cursor, total, origin, hit }`.
- `RecommendationConfig { w_distance, w_security, w_rating, w_freshness, w_verification }` — constants
  in code (§34; Ledger #8), deterministic; missing inputs → 0.5.
- `GetParkingDetails { reader, freshness }` → `ParkingDetails { location, freshness_category, is_open_now }`.

Search criteria parsing/validation (radius allowlist, sort enum, page size clamp, cost/type/security
filters) lives in application as `SearchCriteria::parse`-style constructors — the web layer only
maps HTTP params to these constructors (§7: no business rules in handlers).

New deps: `base64`, (dev) nothing extra. Scoring is a pure function with unit tests covering
determinism, neutrality for missing data, and tie-breaking.

## 6. Infrastructure (crates/infrastructure)

- `geocoding/fake.rs` — `FakeGeocoder` implementing the port (Ledger #2). Landmark table shared via a
  small `devdata` module with the seeder.
- `parking/search.rs` — `SqlxParkingSearchReader`: one hand-written `query!` (or `query_as!`) with
  `ST_DWithin`, filter predicates (`cost_kind = ANY`, `parking_type = ANY`, security all-of via
  `NOT EXISTS (unnest($codes) ...)`, open-now CTE), `ORDER BY` per sort, keyset `WHERE (val, id) > ($v,$id)`
  style predicates, `LIMIT $n`. Keyset on `Recommended` is not used in SQL (Rust-side, see §5) but
  distance/`security`(count of yes-features)/`rating`/`recently_verified` sorts paginate in SQL.
- `parking/details.rs` — `SqlxParkingDetailsReader`: aggregate assembly (location + security + hours)
  in 2–3 explicit queries.
- Compile-time checking needs `DATABASE_URL` → sqlx auto-loads `.env` at crate root; README updated.

`test-support` additions: `ParkingBuilder` (name/type/cost/coords/hours/features/rating/seed),
`SecurityFeature` helpers; `db_test` harness reused unchanged.

## 7. Web layer (crates/web)

Routes (all GET, public):

| Route | Page | Handler |
|---|---|---|
| `GET /` | P1 home | hero + search form; "use my location" (Alpine, browser geolocation → `/search?lat=&lon=`, §22); how-it-works; 4 featured seed locations if present |
| `GET /search` | P2 results | full page; detects `HX-Request: true` → renders only `partials/search_results.html` |
| `GET /parking/{id}` | P3 details | full aggregate; 404 page for unknown/removed |
| `GET /about` | P7 | static content |

Templates: extend `layouts/base.html` (nav, footer, vendor scripts block); `pages/{home,search,parking_details,about}.html`;
`components/{parking_card,freshness_badge,cost_badge,security_badges,hours_table}.html`;
`partials/{search_results,filters,map_panel}.html`.

HTMX: filter/sort form and "load more" use `hx-get="/search"`, `hx-target="#results"`,
`hx-push-url="true"` (§33 shareable URLs). Fragments embed `#search-data` JSON for the Alpine map store.

Alpine/MapLibre: `web/static/js/search.js` — creates map, renders markers from JSON, syncs selection,
"recenter" button. `web/static/css/map.css` holds the MapLibre overrides (§12).

Assets: `package.json` gains `maplibre-gl`, `alpinejs`, `htmx.org` + `"build:assets"` copying dist
files into `web/static/vendor/` (committed). Map tiles = MapLibre demotiles (Ledger #3).

## 8. Seeder

`cargo run -p bikenest-web -- seed-mock` (main dispatches on argv; default = serve):

- Deterministic dataset: ~24 locations around Curitiba landmarks (Rua XV de Novembro, Jardim
  Botânico, Parque Barigui, Mercado Municipal, MON, …) with varied type/cost/hours/security/rating/
  `last_verified_at` spanning all freshness buckets; timezones all `America/Sao_Paulo`.
- Idempotent: single transaction — `DELETE FROM parking_location WHERE seed_key IS NOT NULL` → insert.
- Prints a summary count at the end. Ledger #1 (remove/gate in M7).

## 9. Testing

| Layer | Tests |
|---|---|
| domain | opening-hours `is_open_at` (multi-period, all-day, closed day, unknown, DST tz), cost tri-state distinctions, freshness boundaries, `ParkingType`/currency code validation |
| application | `SearchParking` with fake reader+geocoder: origin resolution precedence, radius clamp, page-size clamp, keyset cursor round-trip, recommended-sort scoring determinism + neutrality + cap, filters forwarded |
| infrastructure (`#[db_test]`) | seeder dataset visible to search query; `ST_DWithin` radius correctness; each filter; security all-of; open-now SQL vs domain computation agreement; keyset pagination stability (insert a new row mid-iteration); details assembly |
| web (`#[db_test]`) | `/` 200; `/search?q=…` full page + HX fragment mode; `/search?lat=&lon=` direct; filters present in URL; `/parking/{id}` 200 + 404; `/about` 200 |

## 10. Task breakdown

1. `0002_parking.sql` migration + security_feature catalog rows; verify `cargo run` applies it.
2. Domain: value objects + `ParkingLocation` + `OpeningHours` + `Freshness` + unit tests.
3. Application: read models, ports, criteria parsing, cursor codec, scoring, `SearchParking`,
   `GetParkingDetails` + tests with fakes. (`cargo add base64` etc. via `cargo add` only.)
4. Infrastructure: `FakeGeocoder`, search/details readers with `query!`, `ParkingBuilder` in
   test-support, `#[db_test]` integration tests.
5. Seeder command + deterministic dataset; run against compose DB.
6. Web: handlers + query-param mapping + HX detection; templates/components/partials; vendor assets;
   `search.js` + `map.css`; Tailwind classes matching the design screens.
7. HTTP tests; README (build-time DB requirement, seed command, assets command); Ledger entries;
   live acceptance walkthrough against `docker compose` + `seed-mock`.

## 11. Risks / notes

- **Build needs DB** (compile-time `query!`): documented in README; offline cache deferred to M7.
- **MapLibre + Alphine bundle size** (~1 MB dev): acceptable locally; minified by vendor step; revisit in M7.
- **Open-now in SQL** has DST edge cases; acceptable for M1 (dataset timezone has no DST), noted for M7.
- **Recommended-sort 500 cap**: pragmatic; replaced by a materialized score column if profiles show need (M7).
- **htmx 4 is fresh software**: pin the exact version (4.0.0) in `package.json`; breaking-change surface for M1 is small (only `HX-Request` detection, GET boosts, no OOB swaps, no delete forms). Watch explicit-inheritance and 4xx-swap behavior when M2+ adds form flows (§116.6).

---

## Ledger additions this milestone

| # | Item | Kind | Introduced | Remove/improve by | Notes |
|---|---|---|---|---|---|
| 13 | `seed_key` column on `parking_location` | mock data support | M1 | M7 (drop column when seeder is dev-flag-gated) | Used to identify/clean mock rows |
