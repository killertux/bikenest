# BikeNest — Implementation Plan

## Purpose

This is the high-level, milestone-based plan for building BikeNest end to end. It is a companion to `REQUIREMENTS.md` (the source of truth for *what* and *how*) and `UI_DESIGN.md` (the source of truth for *pages*).

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
| M4 | M2 (photo moderation queue); P3 gallery + D1/D3 photo-attach reach full form here |
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
- Askama `base` layout + shared components (parking card, filter panel).

**Working app means:** type a destination → coordinates resolve (fake geocoder) → mock parking appears on map + list → filter by cost/type → open a details page. Fully navigable without an account.

**Mocks/fakes:** `FakeGeocoder`, mock parking seed data (both Ledger-tracked).

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

**Working app means:** register → verify (via captured fake email) → log in; log out; seeded admin promotes a user; unverified/suspended users are blocked from contributions.

**Mocks/fakes:** `FakeEmailProvider`, `FakeOAuthProvider` (both Ledger-tracked).

---

## M3 — Community contributions

**Goal:** verified users can grow and correct the dataset.

**Build:**

- Add parking location (with advisory duplicate detection).
- Propose changes to existing parking (history retained).
- Reviews (five-star, one active per user, moderation state).
- Verification signals ("still exists", attribute verification, "I parked here").
- Favorites.
- Rate limiting on contribution endpoints (in-memory first — **Ledger entry**).
- Contribution history + freshness calculation (Fresh/Recently/Aging/Stale/Very stale).
- Web forms + HTMX interactions for all of the above.

**Working app means:** a verified user adds a location (duplicate warning shown), proposes an edit, reviews it, verifies it, favorites it — all persisted and visible.

**Mocks/fakes:** in-memory rate limiter (Ledger-tracked).

---

## M4 — Photos

**Goal:** the photo pipeline from upload to moderated publication.

**Build:**

- Image-storage port + local-filesystem implementation — **Ledger entry**.
- Upload validation (size, dimensions, content sniffing), re-encode, EXIF stripping, thumbnails.
- Photo moderation state (`PENDING_REVIEW`/`APPROVED`/`REJECTED`).
- Moderation queue page + approve/reject actions.
- Photo gallery on the details page (approved photos only).

**Working app means:** upload a photo → it appears in the moderator queue and is NOT public → moderator approves → photo shows on the details page; rejection works; a test asserts EXIF metadata is gone.

**Mocks/fakes:** local filesystem image storage (Ledger-tracked).

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

**Working app means:** a user exports their data, requests deletion, and the account is anonymized while community contributions remain unattributed; privacy pages are versioned; retention jobs are testable.

**Mocks/fakes:** none new.

---

## M7 — Hardening & production readiness

**Goal:** a production-deployable, observable, accessible, localized application.

**Build:**

- Security headers + CSP; htmx-4 error-response swap handling (§116.6).
- Structured logging; separate diagnostic logs vs audit events; log retention.
- Replace fakes with real providers: geocoder, map/tile provider, email (SMTP/ESP), object storage, Google OAuth (real credentials) — **clears the corresponding Ledger entries**.
- E2E browser tests for critical journeys; accessibility pass (WCAG 2.2 AA) incl. keyboard-only; i18n (pt-BR + en); SEO (titles/meta/canonical/sitemap/robots); stable URLs.
- Deployment architecture, backups, restore, disaster recovery, performance-target validation.

**Working app means:** a production-like deployment is documented; E2E is green; language is switchable; backups and restore are configured and tested.

**Mocks/fakes:** all Ledger fakes removed or explicitly gated behind a dev flag.

---

## Cross-cutting conventions

- **Mock data** is always introduced via a CLI command or an env-gated flag, never silently in production code paths. It is always a Ledger entry.
- **Fakes** implement the same ports as real providers, so replacement is a wiring change, not a domain change (per §84).
- **Every milestone** is expected to leave the test suite green and the README's "run locally" instructions current.

---

## Ledger

Bookkeeping for anything temporary, mocked, or knowingly incomplete. **Each entry must be removed/improved by its target milestone.** Append new entries here as they arise; do not let mock data or fakes ship unnoticed.

| # | Item | Kind | Introduced | Remove/improve by | Notes |
|---|---|---|---|---|---|
| 1 | Mock parking seed data (`seed-mock`) | mock data | M1 | M7 (gate behind dev flag) | Production must start empty (§116.1). Keep as a dev-only command. |
| 2 | `FakeGeocoder` | fake | M1 | M7 | Replace with real geocoding provider; keep a fake for tests. |
| 3 | Mock map tile usage in dev | mock data | M1 | M7 | Never point production at public OSM tiles (§83); choose a provider. |
| 4 | `FakeEmailProvider` (capturing) | fake | M2 | M7 | Replace with SMTP/ESP; keep capture mode for tests. |
| 5 | `FakeOAuthProvider` (Google stub) | fake/stub | M2 | M7 | Replace with real Google OAuth client + credentials. |
| 6 | In-memory rate limiter | stub | M3 | M7 | Replace with shared/Redis-backed store if multi-instance. |
| 7 | Local filesystem image storage | stub | M4 | M7 | Replace with object storage (S3-compatible). |
| 8 | Hardcoded recommendation weights | improve | M1 | M7 | Make configurable in application code (§34). |
| 9 | Hardcoded freshness thresholds | improve | M3 | M7 | Make configurable (§40). |
| 10 | `seed-admin` command | improve | M2 | M7 | Ensure idempotent + secret-safe in production. |
| 11 | Hardcoded user-facing strings | improve | M0–M6 | M7 | Extract to i18n catalog (pt-BR + en). |
| 12 | _reserved for future entries_ | — | — | — | — |
