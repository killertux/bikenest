# PENDING_FOR_PRODUCTION.md

> **Purpose.** Everything BikeNest still needs before a production launch that is deliberately
> **not part of M7**. M7 is hardening-only: security headers + CSP, structured logging, E2E +
> accessibility, SEO/i18n finish, and the deployment/backup/DR/incident-response docs. This file is
> the "beyond M7" backlog — mostly **replacing fakes/dev implementations with real providers**, plus
> items blocked on legal/product review and optional features we revisit only if a need materializes.
>
> Companion to `PLAN.md` (Ledger), `plans/m7-hardening.md` (what M7 *does* do), and
> `REQUIREMENTS.md`. Keep this file current as items land or get re-scoped.

---

## A. Fakes & dev implementations to replace

Every external dependency ships behind a port (§84), so replacing one is a wiring change + a new
impl, never a domain change. The list below is the *complete* set of fakes/dev impls still wired into
the production path in `crates/web/src/http.rs` (`app_router_with`).

| # | Faker / dev impl | Lives in | Replace with | Blocked on | Notes |
|---|---|---|---|---|---|
| 1 | `FakeGeocoder` (deterministic Curitiba landmarks + fallback) | `crates/infrastructure/src/geocoding.rs`; wired in `http.rs` | `MapboxGeocoder` behind the `Geocoder` port (hosted OSM-derived), selectable via `GEOCODER=mapbox`; `fake` remains the dev/test default | product/ops: Mapbox account + `MAPBOX_ACCESS_TOKEN`; provider contract / DPA / international-transfer review (§83, §C below) | **Done (M8):** `MapboxGeocoder` + `geocoder_from_env()` (`GEOCODER` mapbox\|fake). §77: server-side, query only (no identity). Geocode error → graceful page (not 500); token missing → falls back to `fake`. Ledger #2 | 
| 2 | Demo map tiles (`demotiles.maplibre.org`) | `web/static/js/search.js:11`, `web/static/js/details-map.js:12` | Configurable `MAP_STYLE_URL` + a real tile/style provider (self-hosted vector tiles, Protomaps PMTiles, or a hosted provider e.g. MapTiler); attribution enforced | product/ops: provider choice + licensing/attribution + usage limits + API key | Ledger #3. Both JS files hardcode the style URL today |
| 3 | `FakeOAuthProvider` (Google stub) | `crates/infrastructure/src/auth/oauth.rs`; wired in `http.rs` | `GoogleOAuthProvider` behind `AuthenticationProvider` — real Google OAuth 2.0 + OpenID Connect (`openid email` scope, PKCE) | Google Cloud OAuth client + approved consent screen + redirect URI | Ledger #5. Keep the fake for tests/dev |
| 4 | `LocalDiskStorage` (local-filesystem `ObjectStorage`, HMAC-signed `/media` URLs) | `crates/infrastructure/src/storage.rs`; wired in `http.rs` | `S3ObjectStorage` behind `ObjectStorage` — any S3-compatible target (AWS S3 / Backblaze B2 / Cloudflare R2 / MinIO), presigned GET parity | product/ops: target choice + bucket + credentials + DPA | Ledger #7. §77: opaque keys, no user metadata |
| 5 | `InMemoryRateLimiter` (per-process, per-service instance) | `crates/infrastructure/src/auth/rate_limit.rs`; constructed **4×** in `http.rs` (auth, contributions, photos, moderation) | `ValKeyRateLimiter` behind the `RateLimiter` port, one **shared** instance wired into all services | none | **Done (M8):** `ValKeyRateLimiter` (single `VALKEY_URL` or cluster `VALKEY_CLUSTER_URLS`, atomic Lua sliding-window over a ValKey sorted set). Wired as one shared instance in `app_router_with`. **Fail-open on outage by default** so a ValKey outage doesn't 429 the site (`RATE_LIMIT_FAIL_OPEN`). Ledger #6 |
| 6 | Email `fake` provider (capture + outbox) | `crates/infrastructure/src/email/{fake,smtp,resend}.rs` | Already has real `smtp` (lettre) and `resend` (API) impls — just select a production relay/ESP and supply credentials | ops: production SMTP relay or Resend account + DPA | Ledger #4. Largely done; only the *production credentials* remain |
| 7 | `OfflineTimezoneResolver` (bundled polygon data) | `crates/infrastructure/src/timezone/offline.rs`; wired in `http.rs` | **Keep as primary** (real, deterministic, offline). Re-evaluate against a reverse-timezone provider only if accuracy gaps appear | none (re-evaluation is optional) | Ledger #16. Not a fake — document + keep as fallback |

---

## B. Dev affordances & secrets to gate for production

These are not providers but dev-only conveniences that must be disabled or hardened before go-live.

| Item | Today | Production requirement | Blocked on | Notes |
|---|---|---|---|---|
| `seed-mock` command (24 Curitiba locations + photos) | reachable whenever the binary runs | Refuse to run when `APP_ENV=production` (or equivalent flag) | none — pure code change | Ledger #1; §116.1 production starts empty |
| `seed-admin` command | idempotent, env-driven (`ADMIN_EMAIL`/`ADMIN_PASSWORD`) | Idempotent + `APP_ENV=production` guard + secret-safe password handling | none — pure code change | Ledger #10 |
| `seed_key` column (`parking_location`, `parking_photo`) | exists for idempotent re-seeding | Drop via migration once the seeder is env-gated (or keep if harmless) | after `seed-mock` gating | Ledger #13 |
| `MEDIA_SIGNING_SECRET` | dev-insecure default (`dev-insecure-media-signing-secret`) in `crates/infrastructure/src/storage.rs` | **Required** in production — fail fast at startup if absent/default | none — pure code change | Ledger #14 |
| Demo tiles (again) | see A-2 | remove `demotiles` fallback in production | see A-2 | — |

---

## C. Blocked on legal / product review

Engineering cannot resolve these alone (§71/§112).

| Item | What's needed | Where |
|---|---|---|
| **Policy text** (privacy/terms/cookies) | Final legal wording; currently `seed-policies` placeholder markdown in `policies/*.md` | Ledger #21 |
| **Legal bases** in the data-processing inventory | Review the `⚠ legal review` markings in `docs/data-processing-inventory.md` against the real business model | §68/§69 |
| **Retention periods** requiring approval | Inactive-account anonymization + deleted-shell purge periods (`INACTIVE_ACCOUNT_ANONYMIZE_AFTER_DAYS`, `DELETED_ACCOUNT_PURGE_AFTER_DAYS`) | §75 |
| **Provider contracts/DPAs** | For every real provider in §A: controller/processor role, DPA, transfer mechanism (§76/§77) | §76/§77 |
| **International-transfer assessment** | Region decisions for Google OAuth, email ESP, tile/geocode provider, object storage, hosting | §76 |

---

## D. Optional / revisit-only-if-needed

Not required for the initial release. Revisit only if a measured need arises.

| Item | Trigger to revisit | Reference |
|---|---|---|
| Async export generation (`PROCESSING` worker) | export volume grows beyond synchronous assembly | M6 plan §2 |
| Consent banner / cookie-preference manager | if non-essential/tracking cookies are ever introduced (none today) | §78 |
| Face / license-plate auto-detection in photos | privacy/legal need to redact people/plates in UGC photos | §80 |
| Report appeal / re-open a resolved report | user feedback that moderation outcomes need appeal | §43 |
| Public API | only as a separate product/security decision | §108 |
| Free-text search / separate search engine | only if a measured need over parking names/descriptions appears | §101/§116.8 |
| Turn-by-turn navigation | explicit non-goal; external links only | §104 |

---

## E. Operations & scale

| Item | Notes | Reference |
|---|---|---|
| **Capacity planning** | Expected scale/volume is TBD — revisit before capacity planning | §116.5 |
| **Multi-instance posture** | Sessions are DB-backed (fine); the rate limiter (A-5) is the main blocker to horizontal scaling; object storage + managed DB are prerequisites | §100 |
| **Performance-target validation under load** | §99 targets (< 500 ms page, < 300 ms search) are engineering guidance; a load sanity check ships in M7, real capacity testing is separate | §99 |
