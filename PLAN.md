# BikeNest — Implementation Plan

## Purpose

This is the high-level, milestone-based plan for building BikeNest end to end. It is a companion to `REQUIREMENTS.md` (the source of truth for *what* and *how*) and `UI_DESIGN.md` (the source of truth for *pages*, plus the pointer to the approved visual design in `design-project/`).

Styling is implemented with **Tailwind CSS 4.3** (REQUIREMENTS §12): the token block from `design-project/colors_and_type.css` is mapped into the Tailwind `@theme` in M0, and all subsequent UI work uses Tailwind utilities against those tokens. The `design-project/` export (screens + `DESIGN.md` + `DESIGN-HANDOFF.md`) is the visual contract for every page built from M1 onward.

The governing rule for this plan:

> **Every milestone ends with a running application whose stated journeys can be exercised against real infrastructure (PostgreSQL + PostGIS, real SQLx), not merely a codebase that compiles.**

## Guiding principles

1. **Vertical slices, not horizontal layers.** We build the walking skeleton first, then add capability slices. We do not build "all of the domain, then all of the web layer."
2. **Real infrastructure from day one.** PostgreSQL/PostGIS and SQLx are real from M0. Only *external* providers (geocoding, email, image storage, OAuth) are faked, and only temporarily.
3. **Mock/fake data is a development affordance, never a production feature.** Production has no seed dataset (all user-provided — see `REQUIREMENTS.md` §116.1). Any temporary mock data or fake provider is recorded in the **Ledger** at the bottom of this file with the milestone that must remove or replace it.
4. **Each milestone's "done" is a testable journey**, described explicitly per milestone.

## Milestone overview

| Milestone | Theme | Testable outcome |
|---|---|---|
| M0 | Walking skeleton | Server boots; health/readiness work; real-Postgres test loop; migrations run; a new dev can onboard from the README alone |
| M1 | Core search & map (read-only) | Search a destination, see mock parking on map + list, filter/sort, open a details page — all without auth |
| M2 | Accounts & authentication | Register → verify email → log in/out; seeded admin; sessions + CSRF; roles |
| M3 | Community contributions | Verified users add/edit/review/verify/favorite parking; persisted to real Postgres |
| M4 | Photos | Upload → validate → moderate → publish; moderation queue works |
| M5 | Moderation & reporting | Users report; moderators act; audit trail is written |
| M6 | Privacy & account lifecycle | Export, deletion/anonymization, privacy pages, retention jobs |
| M7 | Hardening & production readiness | Security headers, observability, real providers, E2E, i18n, deployment |

---

## Milestone ↔ page mapping

Which `UI_DESIGN.md` pages first ship in each milestone. A page listed under a milestone is *testable in that milestone*; later milestones may enhance it (e.g. P3 gains photos in M4, i18n/SEO in M7).

| Milestone | Pages first shipped (`UI_DESIGN.md` labels) |
|---|---|
| M0 | base layout, E1 (404), E2 (error) |
| M1 | P1 (home), P2 (search), P3 (details), P7 (about) |
| M2 | A1–A6 (register/login/verify/reset/OAuth), C1 (account overview), C2 (change password), C3 (change email), M5 (user management — role assignment) |
| M3 | C4 (favorites), C5 (contribution history), D1 (add), D2 (edit/propose), D3 (review), D4 (verify) |
| M4 | M2 (photo moderation queue); P3 gallery + D1 photo-attach reach full form here (D3 *review*-photo attach is deferred to M5 — see `plans/m4-photos.md`) |
| M5 | M1 (moderation dashboard), M3 (reports), M4 (proposals), M6 (audit log); M5 user management gains suspend/restore |
| M6 | P4 (privacy), P5 (terms), P6 (cookies), C6 (privacy & data), C7 (export status) |
| M7 | no new pages — hardening, i18n, SEO, security headers applied across all pages |

Notes:

- **P4/P5/P6** may get a minimal placeholder earlier than M6 if legally required for session cookies; the *versioned, complete* pages land in M6.
- **C1** starts minimal in M2 (overview + nav) and grows as C2–C7 arrive.
- **P7** (about) is optional/static and can be deferred to M7 without blocking anything.
- **D4** (verify / "I parked here") lives as modals on P3 rather than standalone routes.
- Moderation page labels M1–M6 in `UI_DESIGN.md` are *page* identifiers and are unrelated to these M0–M7 *milestone* identifiers.

---

## M0 — Walking skeleton

**Goal:** prove the architecture and the dev/test loop end to end before building any user-facing feature.

**Build:**

- Cargo workspace with crates: `domain`, `application`, `infrastructure` (persistence, auth, storage, geocoding, email), `web`, `test-support`.
- `docker-compose.yml`: PostgreSQL + PostGIS, named volume, health checks.
- Migration tooling (`sqlx migrate`) and the first migration (enable PostGIS; minimal `users` table).
- Configuration loader + `.env.example` + README onboarding.
- Frontend tooling: Tailwind CSS 4.3 build pipeline (CSS entry with `@import "tailwindcss"` + `@theme` block mapping the `design-project/colors_and_type.css` tokens), wired into the asset build.
- Health/readiness endpoints (`/healthz`, `/readyz`) wired web → application → infrastructure (real DB ping).
- `test-support` crate: database fixture, transaction-per-test + rollback, SAVEPOINT helper.
- One domain test + one integration test hitting real PostgreSQL (e.g. insert and read a user).

**Working app means:** `docker compose up -d` → `cargo run` serves; `/healthz` returns 200; `/readyz` distinguishes "DB down" from "app error"; `cargo test` runs against real Postgres with rollback isolation; a fresh clone can run everything from the README alone.

**Mocks/fakes:** none yet (Clock, TokenGenerator are trivial real impls).

---

## M1 — Core search & map (read-only)

**Goal:** the read-only product loop over mock data — search a destination and find nearby parking.

**Build:**

- Domain model for `ParkingLocation` (type, cost, security features, opening hours, IANA timezone per §24/§29).
- Full schema: `parking_location` + PostGIS column/spatial index; `security_feature`, `opening_hours`.
- Mock data generator (`cargo run -- seed-mock`) inserting deterministic parking around a city — **Ledger entry**.
- Geocoding port + `FakeGeocoder` (deterministic lat/lon for known addresses) — **Ledger entry**.
- Nearby-search use case (PostGIS `ST_DWithin`/`ST_Distance`), keyset pagination, filters (cost/type/security/radius), sorting (distance + basic "recommended").
- Web: home/search page, search-results page (MapLibre map + list cards), parking-details page, non-map list (accessibility).
- Askama `base` layout + shared components (parking card, filter panel), built with Tailwind utilities implementing the exported `design-project/` screens (`p1`–`p3`, `p7`).

**Working app means:** type a destination → coordinates resolve (fake geocoder) → mock parking appears on map + list → filter by cost/type → open a details page. Fully navigable without an account.

**Mocks/fakes:** `FakeGeocoder`, mock parking seed data (both Ledger-tracked).

**Delivered beyond the original M1 scope** (see `plans/m1-search-map.md` addendum for detail):
`ObjectStorage` port + `LocalDiskStorage` and a `parking_photo` table with a real P3 gallery (pulled
from M4, Ledger #7); full **i18n** runtime — pt-BR + en, `Accept-Language`/cookie resolution, header
toggle (pulled from M7, §102); security-attribute labels moved from a DB catalog table to a hardcoded
domain code list + i18n (migration `0004`); dataset relocated to **Curitiba**; `hx-boost` +
`hx-alpine-compat` for boosted navigation.

---

## M2 — Accounts & authentication

**Goal:** real accounts, secure sessions, and the role model.

**Build:**

- Domain: `User`, `AuthenticationIdentity`, `AccountState` lifecycle, `Role`.
- Schema: `users`, `authentication_identities`, `sessions`, `email_verification_tokens`, `password_reset_tokens`.
- Password hashing (argon2id), server-side sessions (`HttpOnly`/`Secure`/`SameSite=Lax`), CSRF.
- Email/password register/login/logout; email verification; password reset; change password/email.
- Google OAuth behind the `AuthenticationProvider` port (fake first, real in M7) — **Ledger entry**.
- `seed-admin` command (env-driven credential) and audited `GrantRole`/`RevokeRole` use cases.
- Authorization middleware + application-layer role checks (deny-by-default).
- **Rate limiting on the authentication endpoints** — login, registration, password reset, email
  verification and resend (§45). The in-memory `RateLimiter` (Ledger #6) is introduced **here**
  (moved earlier from M3) so authentication never ships without brute-force / account-enumeration
  protection. Limits are keyed per-IP and per-identifier; responses must not leak whether an account
  exists (§45). Contribution-endpoint limits still arrive in M3, reusing the same port.

**Working app means:** register → verify (via captured fake email) → log in; log out; seeded admin promotes a user; unverified/suspended users are blocked from contributions; repeated failed logins / reset requests are throttled.

**Mocks/fakes:** `FakeEmailProvider`, `FakeOAuthProvider`, in-memory `RateLimiter` (all Ledger-tracked).

---

## M3 — Community contributions

**Goal:** verified users can grow and correct the dataset.

**Build:**

- Add parking location (with advisory duplicate detection). A **coordinate→IANA timezone resolver
  port** (§29) replaces M1's static Curitiba mapping now that contributors supply arbitrary
  coordinates; timezone is confirmable/overridable by the contributor.
- Propose changes to existing parking. **Field-level history (§107)** for existence, location, type,
  cost, opening hours, security and moderation state — changes are stored as versioned
  proposals/revisions rather than overwriting, so history can be reconstructed. **Optimistic
  concurrency (§100)** via a `version` column guards against lost updates on concurrent edits.
- Reviews (five-star, one active per user via a unique constraint, moderation state).
- Verification signals ("still exists" / "no longer exists" / "information changed", per-attribute
  verification, "I parked here"). **Conflict resolution (§106):** conflicting signals are preserved
  and surfaced as a `Conflicting` confidence state (Reported / Verified / Recently verified / Stale /
  Conflicting), never silently averaged away; the resolution rule is defined here.
- **Recommendation explanation (§105)** on P3 — "recommended because…" surfaces the scorer's
  per-factor breakdown (distance / security / rating / freshness / verification) without claiming
  certainty the data can't support.
- Favorites.
- Contribution-endpoint rate limiting (§45), reusing the `RateLimiter` port introduced in M2
  (Ledger #6).
- Contribution history + freshness calculation (Fresh/Recently/Aging/Stale/Very stale).
- Web forms + HTMX interactions for all of the above.

**Working app means:** a verified user adds a location (timezone auto-derived; duplicate warning shown), proposes an edit (history retained), reviews it, verifies it (a conflicting signal shows as such, not averaged), sees why it's recommended, favorites it — all persisted and visible.

**Mocks/fakes:** none new (the in-memory `RateLimiter` arrived in M2).

---

## M4 — Photos

**Goal:** the photo pipeline from upload to moderated publication.

> **Pulled forward to M1:** the `ObjectStorage` port + `LocalDiskStorage` (signed, expiring
> `/media` URLs, Ledger #7), the `parking_photo` table and the P3 gallery already exist. M4 is now
> the **upload + processing + moderation** work on top of that foundation, not storage itself.

**Build:**

- Authenticated upload flow (form + HTMX) feeding the existing `ObjectStorage` port.
- Upload validation (size, dimensions, content sniffing — not extension), safe decode, **re-encode**,
  **EXIF stripping**, thumbnail/derivative generation. The publicly served asset MUST be a processed
  derivative, never the original upload (§80). *(M1 seeds pre-approved originals for the demo; this
  step replaces that with the real pipeline.)*
- Flip `parking_photo.moderation_state` default from `APPROVED` (M1 seed convenience) to
  **`PENDING_REVIEW`**; enforce that only `APPROVED` photos are public (already filtered in the
  reader) and that the original is never publicly reachable.
- Moderation queue page (M2 photo queue) + approve/reject actions; photo-upload rate limiting (§45).
- **D3 review-photo attach (§38) is deferred to M5** (rides review moderation, which lands there);
  M4 delivers the location-photo pipeline. See `plans/m4-photos.md` for the full plan and decisions.

**Working app means:** upload a photo → it enters the moderator queue as `PENDING_REVIEW` and is NOT public → moderator approves → the processed derivative shows on the details page; rejection works; a test asserts EXIF metadata is gone and the original is not served.

**Mocks/fakes:** none new (local-filesystem `ObjectStorage` already tracked as Ledger #7).

---

## M5 — Moderation & reporting

**Goal:** a defensible moderation and audit layer over user-generated content.

**Build:**

- Reports with states (`OPEN`/`UNDER_REVIEW`/`RESOLVED`/`DISMISSED`) + report form.
- Moderation actions: hide review/photo, invalidate parking, review proposals.
- Audit events for all moderation, role, and security actions.
- Moderation dashboard, report queue, proposal review, contribution-history inspection.
- Admin audit-log viewer.

**Working app means:** a user reports content; a moderator resolves it; the offending content is hidden; the audit trail shows who did what; a user cannot resolve their own report.

**Mocks/fakes:** none new.

---

## M6 — Privacy & account lifecycle

**Goal:** data-subject rights and compliant account termination.

**Build:**

- Personal-data export (JSON) with expiring, authenticated download links.
- Account deletion/anonymization (retain-anonymized-contributions model per §74).
- Privacy-request workflow (access/rectification/deletion/export/restriction/objection/consent-withdrawal).
- Retention jobs (sessions, tokens, "I parked here", temporary exports/uploads).
- Versioned privacy/terms/cookies pages; cookie inventory.
- Data-processing inventory and legal-basis mapping (legal bases marked for legal review).
- **Third-party provider & international-transfer inventory (§68/§76/§77):** enumerate every
  provider that may receive personal data and document, per provider, what data crosses the boundary —
  Google OAuth, email provider, geocoding provider, map/tile provider, object-storage provider,
  hosting provider, observability/error-tracking. Confirm the §77 minimization boundaries (the map
  renderer/geocoder/image store receive no account identity).

**Working app means:** a user exports their data, requests deletion, and the account is anonymized while community contributions remain unattributed; privacy pages are versioned; retention jobs are testable; the provider data-flow inventory is complete.

**Mocks/fakes:** none new.

---

## M7 — Hardening & production readiness

**Goal:** a production-deployable, observable, accessible, localized application.

**Build:**

- Security headers + **CSP**; htmx-4 error-response swap handling (§116.6). **CSP ↔ Alpine/HTMX
  decision (§64/§65):** the current templates use inline Alpine expressions (`x-data`, `@click`) and
  `hx-boost`, which a strict policy would break (Alpine's default build needs `unsafe-eval`). Adopt
  **either** Alpine's CSP build (pre-registered components, no inline expressions) **or** a
  nonce-based CSP — decided as part of this milestone. Until then, avoid adding new inline-expression
  templates that would deepen the migration. (Tracked as Ledger #15.)
- Structured logging; separate diagnostic logs vs audit events; log retention.
- **Incident-response strategy (§81):** document detection → classification → containment → impact /
  personal-data assessment → escalation → regulatory/user notification → remediation → incident record.
- Replace fakes/dev impls with real providers: geocoder, map/tile provider, email (SMTP/ESP),
  **object storage (S3-compatible, replacing `LocalDiskStorage`; real `MEDIA_SIGNING_SECRET`)**,
  Google OAuth (real credentials) — **clears the corresponding Ledger entries**.
- E2E browser tests for critical journeys; accessibility pass (WCAG 2.2 AA) incl. keyboard-only.
- **i18n finish (§102):** the runtime (bilingual catalog, `Accept-Language` + `lang`-cookie
  resolution, header toggle) shipped in **M1**; M7 adds SEO `hreflang`, audits any strings added by
  M2–M6 features, and confirms locale-aware dates/currency where practical.
- SEO (titles/meta/canonical/sitemap/robots); stable URLs.
- Deployment architecture, backups, restore, disaster recovery, performance-target validation.

**Working app means:** a production-like deployment is documented; E2E is green; a strict CSP is enforced with Alpine still working; language is switchable with `hreflang`; backups and restore are configured and tested.

**Mocks/fakes:** all Ledger fakes/dev impls removed or explicitly gated behind a dev flag.

---

## Cross-cutting conventions

- **Mock data** is always introduced via a CLI command or an env-gated flag, never silently in production code paths. It is always a Ledger entry.
- **Fakes** implement the same ports as real providers, so replacement is a wiring change, not a domain change (per §84).
- **Every milestone** is expected to leave the test suite green and the README's "run locally" instructions current.
- **Text search (§101):** **none.** Search resolves a *destination* (address / place / landmark /
  neighborhood / city / current location) via the geocoder to coordinates, then runs a PostGIS
  proximity query. There is deliberately no free-text search over parking names/descriptions in the
  initial release, and no separate search engine (§101/§101-consistency) — revisit only if a measured
  need arises.
- **i18n (§102):** user-facing strings live in the web-layer catalog (`crates/web/src/i18n.rs`), never
  hard-coded in domain/application logic. Labels for extensible catalogs (parking types, security
  attributes) are keyed by their domain **code**, so adding an attribute is a code + translation, not
  a schema migration.

---

## Ledger

Bookkeeping for anything temporary, mocked, or knowingly incomplete. **Each entry must be removed/improved by its target milestone.** Append new entries here as they arise; do not let mock data or fakes ship unnoticed.

| # | Item | Kind | Introduced | Remove/improve by | Notes |
|---|---|---|---|---|---|
| 1 | Mock parking seed data (`seed-mock`) | mock data | M1 | M7 (gate behind dev flag) | Production must start empty (§116.1). Now 24 **Curitiba** locations + seeded photos. Keep as a dev-only command. |
| 2 | `FakeGeocoder` | fake | M1 | M7 | Curitiba landmark table + deterministic fallback. Replace with real geocoding provider; keep a fake for tests. |
| 3 | Mock map tile usage in dev | mock data | M1 | M7 | MapLibre demo tiles. Never point production at public OSM tiles (§83); choose a provider. |
| 4 | Email providers | fake | M2 | M7 | **Generic `EmailProvider` port (§84)**: `fake` (capture + outbox), `smtp` (lettre) and `resend` (API) impls selected via `EMAIL_PROVIDER`. Dev uses **smtp → Mailpit** (docker-compose, UI `:8025`); the `fake` remains for tests. |
| 5 | `FakeOAuthProvider` (Google stub) | fake/stub | M2 | M7 | Replace with real Google OAuth client + credentials. |
| 6 | In-memory `RateLimiter` | stub | **M2** (moved earlier from M3, §45) | M7 | Auth limits in M2; contribution limits (parking/edit/proposal/review/verification/parked-here) in M3. Replace with shared/Redis-backed store if multi-instance. |
| 7 | `LocalDiskStorage` (local-filesystem `ObjectStorage`) | dev impl | **M1** (moved earlier from M4) | M7 | Signed, expiring `/media` URLs (S3-presign parity). Replace with S3-compatible storage. |
| 8 | Hardcoded recommendation weights | improve | M1 | M7 | Make configurable in application code (§34). |
| 9 | Hardcoded freshness thresholds (review-side) | improve | M3 | M7 | Parking-side thresholds already configurable via `FreshnessConfig` (M1); the review-side thresholds are now exercised by M3's confidence rule (`Verified`/`RecentlyVerified`/`Stale`). |
| 10 | `seed-admin` command | improve | M2 | M7 | Ensure idempotent + secret-safe in production. |
| 11 | Hardcoded user-facing strings | improve | M0 | M7 | **Largely resolved in M1**: catalog + `Accept-Language`/cookie resolution + toggle (§102). M7 = SEO `hreflang` + audit strings added by M2–M6. |
| 13 | `seed_key` column on `parking_location`/`parking_photo` | mock data support | M1 | M7 (drop when seeder is dev-flag-gated) | Identifies mock rows for idempotent re-seeding and cleanup. |
| 14 | `MEDIA_SIGNING_SECRET` dev default | improve/secret | M1 | M7 | Local `/media` URL signing uses a dev-insecure default; set a real secret in production. |
| 15 | CSP ↔ inline Alpine/HTMX interaction | risk | M1 | M7 | Inline `x-data`/`@click` + `hx-boost` need `unsafe-eval`; a strict CSP requires Alpine's CSP build or nonces — decide in M7, don't deepen inline usage meanwhile (§64/§65). |
| 16 | `OfflineTimezoneResolver` (bundled polygon data) | improve/dev | M3 | M7 | Real offline coordinate→IANA resolver replacing M1's static Curitiba mapping; re-evaluate against a provider reverse-timezone, keep as fallback. |
| 17 | Confidence-state thresholds + conflict rule hardcoded in domain | improve | M3 | M7 | Make configurable like `FreshnessConfig`; document for tuning (§106). |
