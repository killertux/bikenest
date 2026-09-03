# M7 — Hardening & production readiness — implementation plan

> **Status: planned.** Derived from `PLAN.md` (M7) and `REQUIREMENTS.md`
> (§57, §62–§65, §81, §83–§87, §91, §97–§100, §102, §109–§111, §114,
> §116.6). Parent plan: `PLAN.md`.

Companion to `PLAN.md` (M7 overview) and `UI_DESIGN.md`/`design-project/`. M7 ships **no new
pages** — it hardens, observes, localizes, documents and productionizes everything M0–M6 built. The
visual contract is unchanged; the deliverable is a deployable, observable, accessible, localized,
documented application. (Replacing the Ledger fakes with real providers is **out of M7** — see `PENDING_FOR_PRODUCTION.md`.)

**What already exists (M0–M6, to build on, not rebuild):**

- Health/readiness (`/healthz`, `/readyz`) with DB-down vs app-error distinction (§87); a
  configurable `probe_timeout`.
- The **i18n runtime** (`crates/web/src/i18n.rs`, pt-BR + en, `Accept-Language` + `lang` cookie,
  header toggle) shipped in M1 and extended every milestone since (§102, §116.7). The M7 gap is
  **SEO `hreflang`**, locale-aware dates/currency, and a final audit of M2–M6 strings.
- **Audit events** as a DB table + `AuditLog`/`AuditLogReader` (M2/M5) — already the *separate*
  audit concept that §86 requires. Diagnostic logs (stdout) are the other half, which M7 upgrades.
- All external providers sit behind **ports** (§84): `Geocoder`, `EmailProvider`,
  `AuthenticationProvider`, `ObjectStorage`, `RateLimiter`, `PasswordHasher`, `TokenGenerator`,
  `Clock`, `TimezoneResolver`. Replacing a fake is therefore wiring + a new impl, not a domain
  change.
- Email already has **real** transports: `smtp` (lettre) and `resend` (API) selected via
  `EMAIL_PROVIDER` (Ledger #4); the `fake` remains test-only. Dev uses SMTP → Mailpit.
- `OfflineTimezoneResolver` (Ledger #16) is a real, deterministic, offline coordinate→IANA
  resolver — not a fake.
- A vendored frontend (htmx 4, hx-alpine-compat, Alpine 3, MapLibre 4, Tailwind 4) with **no CDN**
  dependency — already the correct CSP posture (§65 "avoid unnecessary third-party scripts").
- A green test suite across domain/application/infrastructure/web (≈229 tests) and a
  transaction-per-test harness (§113 philosophy).

**Goal:** a production-deployable, observable, accessible, localized application. Strict security
headers (incl. CSP) are enforced with Alpine still working; diagnostic logging is structured and
retained; E2E browser tests cover the critical journeys;
an accessibility pass targets WCAG 2.2 AA; SEO/i18n finish; deployment + backups + restore + DR are
documented and testable; and the incident-response runbook exists.

**Working app means (acceptance):**

- Every response carries the security-header set; a **strict, nonce-free CSP** (`script-src 'self'`,
  no `'unsafe-eval'`) is enforced and Alpine still drives the interactive bits (CSP build).
- The stored-XSS vector in the search map-JSON blob is fixed (see §2/§3 — fixed **first**).
- `RUST_LOG`/JSON structured logging is on; a per-request `TraceLayer` logs method/path/status/
  latency **without** cookies, tokens or PII; log retention is documented.
- Playwright E2E is green on the critical journeys (register→verify→search→add→review; moderation;
  report); an axe-core accessibility scan passes AA on those pages; keyboard-only works.
- `robots.txt` + `sitemap.xml` + canonical + meta description + `hreflang` exist; account/admin/
  moderation pages are `noindex`.
- `docs/deployment.md` (architecture, TLS, secrets, migrations, deploy/rollback),
  `docs/backups.md` (frequency, retention, encryption, restore, §98 re-anonymization note),
  `docs/incident-response.md` (§81) exist; restore is exercised at least once.
- Hardcoded tuning constants (recommendation, confidence, photo, moderation, retention) are
  configurable with documented defaults; the Ledger is cleared of #8, #9, #11, #15, #17–#20 — the
  provider/dev-impl entries move to `PENDING_FOR_PRODUCTION.md`.
- `cargo test` green; fresh-clone onboarding from README still works.

---

## 1. Scope

### In scope

| Area | Content |
|---|---|
| Security headers | Middleware (or `tower-http` layer) adding `Strict-Transport-Security`, `Content-Security-Policy`, `X-Content-Type-Options`, `Referrer-Policy`, `Permissions-Policy`, `X-Frame-Options`/`frame-ancestors` (§64/§65); HSTS only when TLS is on (config-gated) |
| CSP ↔ Alpine/HTMX | Resolve Ledger #15: **Alpine CSP build** + pre-registered components, removing all inline `x-data`/`@click`/`:attr` expressions; a strict `script-src 'self'` CSP (see §3). htmx 4 needs no `unsafe-eval` (no `hx-on`/inline handlers in use) |
| Stored-XSS fix | Replace the `\|safe` JSON blob in `search_results.html` with HTML-safe JSON embedding (escape `<`/`>`/`&`/U+2028/U+2029) — **done first**, independent of the rest |
| Observability | JSON structured logging; `TraceLayer` request logs with a cookie/authorization sanitizer; log-retention policy; keep diagnostic logs vs audit events separate (§86) |
| E2E + accessibility | Playwright suite for critical journeys; axe-core AA scan; keyboard-only pass (§57/§63/§114) |
| SEO + i18n finish | `robots.txt`, `sitemap.xml`, canonical, meta description, OG basics, `hreflang`; `noindex` on private pages; locale-aware dates/currency; string audit (§102/§109/§110) |
| Deployment/ops | Production `Dockerfile`; `docs/deployment.md`, `docs/backups.md`, `docs/incident-response.md`; §97/§98/§81 |
| Configurable constants | `RecommendationConfig`, review/confidence thresholds, photo constants, moderation constants, retention TTLs → `Config`/env (clears Ledger #8/#9/#17/#18/#19/#20) |

> **Provider replacement and production gating are out of M7.** Real geocoder/tiles/S3/OAuth, the
> shared rate limiter, and `seed-mock`/`seed-admin`/`MEDIA_SIGNING_SECRET` gating live in
> `PENDING_FOR_PRODUCTION.md` (§A–§B). M7 keeps the existing fakes/dev impls unchanged.

### Explicitly out of scope (deferred / not required for initial release)

| Item | Where it lands |
|---|---|
| Consent banner / cookie-preference manager | not required — no non-essential cookies (§78) |
| Face/license-plate auto-detection in photos | not required (§80) |
| Async export `PROCESSING` worker | only if volume grows (M6 decision) |
| A public API | explicitly out (§108) |
| Separate search engine / free-text search | explicitly out (§101/§116.8) |
| Turn-by-turn navigation | explicitly out (§104) |
| Report appeal / re-open flow | not required (§43) |
| Horizontal autoscaling beyond the documented single/multi-instance posture | Redis limiter + S3 + managed DB make multi-instance *possible*; the deploy doc states the initial posture (§100) |
| Legal review of policy text (§71) | product/legal — unchanged from M6 |
| **All fakes → real providers + production gating** (geocoder, tiles, S3, OAuth, Redis rate limiter, `seed-mock`/`seed-admin`, `MEDIA_SIGNING_SECRET`) | `PENDING_FOR_PRODUCTION.md` §A–§B |

> Anything in `PENDING_FOR_PRODUCTION.md` (fakes, legal review, optional features, ops/scale) is
> explicitly **not** M7 work.

---

## 2. Decisions

| Decision | Choice | Reasoning |
|---|---|---|
| **CSP strategy** | **Alpine CSP build** with pre-registered components; a **nonce-free** CSP (`script-src 'self'`, `object-src 'none'`, `base-uri 'self'`, `frame-ancestors 'none'`, `form-action 'self'`). `style-src 'self' 'unsafe-inline'` retained **only** because MapLibre injects inline styles (controls/attribution/markers); `img-src`/`connect-src`/`worker-src` whitelist the tile + geocode domains from config | Alpine's default build evaluates expressions with `new Function` → needs `unsafe-eval`, which a nonce does **not** avoid. The CSP build evaluates only property access + method calls, so a strict `script-src` is achievable. htmx 4 + `hx-boost`/`hx-alpine-compat` need no `unsafe-eval` (no `hx-on`, no inline handlers — verified). `style-src 'unsafe-inline'` is a far lower risk than `script-src 'unsafe-eval'` and is a MapLibre constraint |
| **Stored-XSS fix** | `map_json` is serialized then HTML-escaped at the `<`/`>`/`&`/U+2028/U+2029 level (e.g. `\u003c`/`\u003e`/`\u0026`) before embedding, so `JSON.parse` still yields the original value but `</script>` cannot break out. A regression test asserts a name containing `</script><script>` round-trips safely | §103 + the only `\|safe` in the codebase embeds UGC (parking `name`/`address`) into a `<script type="application/json">` block. `serde_json` does **not** escape `<`. This is a stored-XSS vector and is fixed before anything else |
| **Structured logging** | `tracing-subscriber` `.json()` (or `tracing-bunyan-formatter`) in production, human-readable in dev (`RUST_LOG`/`APP_ENV` select). `tower_http::TraceLayer` with a `MakeSpan`/`OnResponse` that logs method/path/status/latency and **redacts** `cookie`/`authorization`/`x-csrf-token` | §86: structured, no secrets, and *separate* from the DB audit trail (which already exists and is unchanged) |
| **E2E framework** | **Playwright** (Node) driving the Docker-composed app, with `@axe-core/playwright` for accessibility. A single `web/e2e/` suite; run via `npm run e2e` | The repo already has a Node toolchain (Tailwind); Playwright is the standard for both E2E and axe-core AA scanning. Keeps the Rust test suite focused on unit/integration/HTTP (§57: E2E = critical journeys only, not duplication) |
| **SEO/indexing** | Public parking pages indexable (canonical, meta description, sitemap); `sitemap.xml` generated from ACTIVE public parking; account/admin/moderation/export pages get `X-Robots-Tag: noindex`; `robots.txt` allows crawling public pages | §109/§110. Private data must never be indexable — the export download already sets `noindex`; M7 extends it uniformly |
| **i18n finish** | `hreflang` alternate links (`pt-br`/`en`) on every public page; a small locale-aware date/currency formatter in the web layer; audit the catalog for M2–M6 strings (most are already cataloged) | §102 "rendered appropriately for the locale where practical" |
| **Migrations in production** | Keep forward-only SQLx migrations; in production run `cargo run -p bikenest-web -- migrate` (or a dedicated command) as an **explicit deploy step**, not on server startup | §90/§97. Startup auto-migrate is the dev convenience; production needs a controlled, reversible-with-restore step |
| **Backups + deletion (§98)** | Daily logical backup (pg_dump) + continuous WAL archiving (managed or self-hosted); retention 30d (configurable); encrypted at rest; **restore procedure includes re-running the `retention` command (anonymize + purge) so restored backups do not resurrect deleted accounts** | §97/§98. Documented + exercised once in a sandbox; RPO/RTO stated as targets (§99 is engineering guidance) |
| **Incident response** | `docs/incident-response.md` implementing the §81 9-step flow (detection→classification→containment→impact→PII assessment→escalation→notification→remediation→record); incident records stored as audit events + a protected doc location | §81. The audit/log basis already exists; M7 writes the runbook and adds the "record" step to the audit trail |
| **Configurable constants** | Add `RecommendationConfig`, `ConfidenceConfig`, `PhotoConfig`, `ModerationConfig`, `RetentionPolicy` → `Config` with documented defaults; keep the current values as the defaults | Clears Ledger #8/#9/#17/#18/#19/#20; §99 says avoid premature optimization, so these are config + docs, not a tuning exercise |
| **Compile-time SQL** | Continue `query_as!`/`query!` (§9/§305) | established M0–M6 |

---

## 3. Security headers + CSP (the deepest change)

### 3.1 Header set (a single middleware, applied to all responses)

```text
Strict-Transport-Security: max-age=31536000; includeSubDomains   (only when TLS is on — config)
Content-Security-Policy:   <see below>
X-Content-Type-Options:     nosniff
Referrer-Policy:            strict-origin-when-cross-origin
Permissions-Policy:         camera=(), microphone=(), geolocation=(self), interest-cohort=()
X-Frame-Options:            DENY            (legacy; CSP frame-ancestors 'none' is the real guard)
```

- `Referrer-Policy: strict-origin-when-cross-origin` — sensible default, no referrer leaks on
  downgrade.
- `Permissions-Policy` disables camera/mic and scopes geolocation to self (the search "use my
  location" feature is the only consumer).
- Headers are applied **before** `hx-boost` swaps matter (they ride the response, not the page).

### 3.2 CSP (proposed, final domains templated from config)

```text
default-src 'self';
script-src 'self';
style-src 'self' 'unsafe-inline';          # MapLibre injects inline styles (attribution/controls)
img-src 'self' data: blob: {TILE_HOST};
font-src 'self';
connect-src 'self' {TILE_HOST} {GEOCODE_HOST};
worker-src 'self' blob:;                    # MapLibre web workers
object-src 'none';
base-uri 'self';
frame-ancestors 'none';
form-action 'self';
```

- No `'unsafe-eval'` — this is the whole point of the Alpine CSP build.
- `style-src 'unsafe-inline'` is a known, accepted MapLibre constraint; we do **not** add inline
  `<style>` of our own, so the surface is MapLibre's only.
- `{TILE_HOST}`/`{GEOCODE_HOST}` come from the provider config (§5); in dev they may be empty.

### 3.3 Alpine CSP-build migration (Ledger #15)

Current inline Alpine usage: **34 occurrences across 6 templates** (`base.html` mobile menu,
`home.html`, `search.html` filters, `parking_new.html`/`parking_edit.html`, `parking_details.html`
lightbox/verify). The migration:

1. Swap the vendored `alpine.min.js` (default build) → the **CSP build** (via the existing
   `npm run build:assets` step), which forbids `eval`/`new Function`.
2. Add `web/static/js/app.js` that registers components with `Alpine.data('bikenest.…', () => ({ … }))`:
   `mobileMenu`, `searchFilters`, `parkingForm`, `detailsPanel` (lightbox/verify/favorite), `homeHero`.
   Each exposes **data + methods** (e.g. `toggle()`), never inline expressions.
3. Rewrite the 34 usages: `x-data="{ open:false }"` → `x-data="bikenest.mobileMenu"`;
   `@click="open = !open"` → `@click="toggle"`; `:aria-expanded="open"`/`x-show="open"` are already
   plain property access and stay. Ternary/object-literal `:class` bindings become precomputed
   component properties.
4. `hx-alpine-compat` continues to re-init Alpine after `hx-boost` swaps — no change.

A page-level smoke test asserts the CSP header is present, `script-src` has no `'unsafe-eval'`, and
the mobile-menu toggle + lightbox still function (Playwright).

### 3.4 The stored-XSS fix (do this first, independent of the rest)

`templates/partials/search_results.html` currently renders
`<script type="application/json" id="search-data">{{ results.map_json|safe }}</script>`. `map_json`
contains `CardVm.name`/`CardVm.address` — UGC (§103). Fix in `view.rs`: serialize, then replace
`<` → `\u003c`, `>` → `\u003e`, `&` → `\u0026`, U+2028 → `\u2028`, U+2029 → `\u2029` before
handing to the template; `JSON.parse` decodes these back losslessly. Add a regression test
(HTTP layer) that seeds a parking named `</script><img src=x onerror=alert(1)>` and asserts the
rendered search fragment contains no literal `</script>`.

---

## 4. Observability (§86)

- **Format:** `tracing-subscriber` with JSON output in production (`.json()` feature or
  `tracing-bunyan-formatter`); human-readable `fmt()` in dev, selected by `APP_ENV`/`RUST_LOG`.
- **Request logging:** `tower_http::TraceLayer` (the `trace` feature is already a dependency) with a
  custom span: `method`, `path` (with `{id}` params elided), `status`, `latency_ms`, `request_id`.
  The `OnRequest`/`OnResponse` hooks **redact** `cookie`, `authorization`, `x-csrf-token` (and any
  future sensitive header) — no session data in logs.
- **Event coverage:** confirm the existing `tracing` calls cover §86's list (auth failures, provider
  failures, DB errors, parking creation, moderation, privacy requests, security events); add `info!`/
  `warn!` where missing. No password/token/cookie/PII ever in a log field — a test greps emitted
  logs for the redaction invariants.
- **Separation:** diagnostic logs (stdout, short-lived) vs audit events (DB, long-lived) remain two
  distinct systems; the M7 docs state this explicitly.
- **Retention:** documented in `docs/deployment.md` (e.g. container stdout → log driver/aggregator,
  N days; or a sidecar). No log-aggregator dependency is added to the codebase itself.

---

## 5. Real providers — **deferred, not M7**

Replacing the fakes/dev impls with real providers (geocoder, tiles, S3, OAuth, shared rate limiter)
and the production gating (`seed-mock`/`seed-admin`/`MEDIA_SIGNING_SECRET`) is tracked in
**`PENDING_FOR_PRODUCTION.md`** (§A–§B), not here. The ports (§84) already exist, so this is pure
wiring + new impls when the product/ops decisions (vendor, credentials, DPAs) are made. M7 ships with
the existing fakes/dev impls unchanged.

---

## 6. E2E + accessibility (§57/§63/§114)

- `web/e2e/` Playwright suite (`playwright.config.ts`, chromium) against `docker compose up`:
  1. register → verify (read the Mailpit-captured link or `fake` outbox) → login → search →
     open a parking → add a location → review it → favorite.
  2. moderation: seeded moderator approves a pending photo / resolves a report.
  3. report: a user reports a review; assert it's not visible to others.
  4. account deletion: export → delete → confirm anonymized/unattributed.
  5. CSP/Alpine smoke: mobile menu + lightbox work under the strict CSP.
- **Accessibility:** `@axe-core/playwright` scan on home/search/details/add/login; fix violations to
  WCAG 2.2 AA; a keyboard-only pass on the search→details→review journey (the map always has the
  list alternative — §63 already satisfied by P2's non-map list).
- The suite runs via `npm run e2e`; it is *critical-journey only* and does not duplicate the Rust
  HTTP tests (§57).

---

## 7. SEO + i18n finish (§102/§109/§110/§111)

- `robots.txt` (allow public, disallow `/account`, `/admin`, `/moderation`, `/media`? no — media is
  fine, but `/account*`/`/admin*`/`/moderation*` disallowed).
- `sitemap.xml` generated from ACTIVE public parking (`/parking/{id}`) + static pages; stable URLs
  (§111) already hold (`/parking/{id}`).
- Per-page: canonical `<link>`, `<meta name="description">`, minimal OG tags; `<link rel="alternate"
  hreflang="pt-br|en">` on public pages.
- `X-Robots-Tag: noindex` uniformly on account/admin/moderation/export pages (extend the M6 export
  download header).
- Locale-aware date/currency formatter in the web layer (prices already carry a currency code; render
  `pt-BR`/`en` number formats); audit i18n catalog for any M2–M6 strings still hardcoded.

---

## 8. Deployment, backups, DR (§97/§98/§99)

- **Production `Dockerfile`** (multi-stage: build workspace → slim runtime; static assets +
  `web/static` baked in). Currently only the dev `docker-compose.yml` exists.
- `docs/deployment.md`: hosting (container), PostgreSQL/PostGIS (managed or self-hosted), object
  storage, TLS termination (reverse proxy/load balancer), secrets (env/secret manager), email, OAuth,
  map/geocoding, migrations (explicit deploy step), observability, health checks (`/healthz`/`/readyz`
  wired to the LB), deploy strategy (rolling), rollback strategy (previous image; forward-only
  migrations → rollback = redeploy + restore if a migration was involved).
- `docs/backups.md`: daily logical backup + WAL archiving; retention (default 30d); encryption at
  rest; restore procedure; RPO/RTO targets (engineering targets per §99); **§98 interaction**:
  restore re-runs the `retention` command (anonymize + purge) so deleted accounts are not
  resurrected; backup access controls documented.
- Performance targets (§99) re-stated and validated: server-rendered page < 500 ms, nearby search
  < 300 ms (PostGIS indexes already present — M1), mutation < 500 ms; a lightweight load sanity check
  (not a benchmark harness) confirms no regressions.

---

## 9. Incident response (§81)

`docs/incident-response.md` implements the 9-step flow: Detection → Classification → Containment →
Impact assessment → Personal-data assessment → Internal escalation → Regulatory/user notification
(when legally required) → Remediation → Incident record. Incident records are written to the audit
trail (new `incident.*` action codes) and protected (admin-only); the runbook names owners, time
boxes, and notification triggers. The audit/log basis already exists (M2/M5/M6).

---

## 10. Configurable constants (Ledger #8/#9/#17/#18/#19/#20)

Add to `Config` + `.env.example` (current hardcoded values become the defaults):

| Config | Current default | Clears |
|---|---|---|
| `RecommendationConfig` (weights per factor) | M1 defaults | #8 |
| `ConfidenceConfig` (review/verification thresholds + conflict rule) | M3 defaults | #9, #17 |
| `PhotoConfig` (10 MiB, 20 MP, JPEG q85, 400 px thumb, rate limits) | M4 defaults | #18 |
| `ModerationConfig` (report length, rate limits) | M5 defaults | #19 |
| `RetentionPolicy` (reset 1h, verification 24h, session 30d/90d, parked-here 90d, export 24h, upload-orphan 24h) | §75 defaults | #20 |

The recommendation config plumbs through `SearchParking` (currently
`DEFAULT_RECOMMENDATION_CONFIG`). (`seed-mock`/`seed-admin`/`MEDIA_SIGNING_SECRET` gating is
deferred — `PENDING_FOR_PRODUCTION.md` §B.)

---

## 11. Testing

| Layer | Tests |
|---|---|
| web (HTTP) | security headers present on a representative set of responses (public, account, admin); CSP has no `'unsafe-eval'`; HSTS absent in dev / present with `TLS_ON`; **stored-XSS regression** (name with `</script>` does not appear literal in the search fragment); `noindex` on account/admin/export; `robots.txt`/`sitemap.xml` served; `hreflang` present on public pages |
| application | config plumbing (RecommendationConfig/ConfidenceConfig/PhotoConfig/ModerationConfig/RetentionPolicy override defaults) |
| E2E (Playwright) | the five critical journeys + CSP/Alpine smoke (§6) |
| accessibility | axe-core AA scan on home/search/details/add/login; keyboard-only on search→details→review |
| security (§60/§61/§86) | log redaction (no cookie/token/PII in emitted logs); audit vs diagnostic separation; §77 boundary asserts unchanged |

---

## 12. Task breakdown

1. **Stored-XSS fix** (search map-JSON embedding + regression test) — ship as a hotfix before the
   rest of M7.
2. Security-headers middleware + CSP (with config-templated tile/geocode hosts) + HSTS gating.
3. Alpine CSP build: swap vendor file, add `app.js` components, rewrite the 34 inline usages.
4. Observability: JSON logging + `TraceLayer` with redaction; log-retention note in docs.
5. Configurable constants (RecommendationConfig/ConfidenceConfig/PhotoConfig/ModerationConfig/
   RetentionPolicy).
6. SEO + i18n finish (robots, sitemap, canonical/meta/OG, hreflang, noindex, locale formatter,
   string audit).
7. E2E (Playwright) + axe-core accessibility + keyboard pass.
8. Production `Dockerfile` + `docs/deployment.md`, `docs/backups.md`, `docs/incident-response.md`;
   exercise restore once in a sandbox.
9. README (new env, E2E, deployment pointers); `PLAN.md` M7 status + Ledger sweep; commit M6 first
   if still uncommitted (see Risks).

> Provider replacement + production gating is a separate track — see `PENDING_FOR_PRODUCTION.md`.

---

## 13. Risks / notes

- **CSP/Alpine regression is the riskiest change** — rewriting 34 inline expressions can silently
  break the mobile menu, filters, or lightbox. Mitigate with the Playwright CSP smoke test and by
  doing it as its own step after headers exist.
- **`style-src 'unsafe-inline'`** is retained for MapLibre; do not add our own inline styles or the
  CSP weakens further. If MapLibre later supports non-inline styling, tighten.
- **The stored-XSS fix must not wait for M7's tail** — it is a live injection vector in the current
  build; land it first (hotfix).
- **Forward-only migrations in prod** — rollback is "redeploy previous image + restore", never a
  down-migration. The deploy doc makes this explicit.
- **Provider replacement is out of scope here** — fakes/dev impls stay for M7; the real-provider
  cutover and its product/ops decisions live in `PENDING_FOR_PRODUCTION.md` (§A–§C).
- **Log redaction must be at the `TraceLayer` hooks**, not just "be careful" — a cookie header
  logged once is a session-hijack primitive (§86). Test asserts it.
- **M6 is uncommitted** at the time of writing (working-tree changes + untracked
  `privacy`/`policy` files). Commit M6 (and its green suite) before starting M7 so the hardening
  diff is reviewable against a clean baseline.

---

## Ledger changes this milestone

M7 clears the **code/tuning** ledger entries. The **provider/dev-impl** entries (#1–#7, #10, #13,
#14, #16) move to `PENDING_FOR_PRODUCTION.md` and are **not** cleared here.

| # | Item | Resolution |
|---|---|---|
| 8 | Hardcoded recommendation weights | `RecommendationConfig` |
| 9 | Hardcoded freshness thresholds | `FreshnessConfig`/`ConfidenceConfig` |
| 11 | Hardcoded user-facing strings | M7 i18n audit + hreflang + locale formatter |
| 15 | CSP ↔ Alpine/HTMX | Alpine CSP build + strict CSP shipped |
| 17 | Hardcoded confidence thresholds | `ConfidenceConfig` |
| 18 | Hardcoded photo constants | `PhotoConfig` |
| 19 | Hardcoded moderation constants | `ModerationConfig` |
| 20 | Hardcoded retention TTLs | `RetentionPolicy` via env |

**Moved to `PENDING_FOR_PRODUCTION.md`** (not M7): #1 `seed-mock` gating, #2 `FakeGeocoder`, #3 demo
tiles, #4 email production credentials, #5 `FakeOAuthProvider`, #6 in-memory rate limiter, #7
`LocalDiskStorage`, #10 `seed-admin`, #13 `seed_key` column, #14 `MEDIA_SIGNING_SECRET`, #16
`OfflineTimezoneResolver` re-evaluation. #21 (policy legal text) stays a product/legal placeholder
(`PENDING_FOR_PRODUCTION.md` §C).
