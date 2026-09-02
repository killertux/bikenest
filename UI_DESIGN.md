# BikeNest — UI Design Specification

## Purpose

This file specifies every page and shared component the application needs, so a designer can produce mockups/design files for each page and the development stage can implement against them. It is a companion to `REQUIREMENTS.md` (behavior/rules) and `PLAN.md` (build order).

The page specs use one consistent template so each page is described in the same shape.

## Implemented design source of truth (`design-project/`)

A complete visual design for this application has been produced and exported in the `design-project/` folder (Open Design export). The visual implementation MUST be built from this design rather than from a loose reinterpretation of this document.

- **Entry point:** `design-project/ui_kits/app/index.html` (see `DESIGN-MANIFEST.json` for the machine-readable screen map and `DESIGN-HANDOFF.md` for the implementation contract).
- **Screens:** 33 exported HTML screens covering this document's page inventory — public `p1`–`p7`, auth `a1`–`a5`, account `c1`–`c7`, contribution `d1`–`d3`, moderation `m1`–`m6`, errors `e1`–`e2` (file names match the page codes below, e.g. `p3-parking-details.html` ↔ P3).
- **Design system:** `design-project/DESIGN.md` (theme, color/type/spacing tokens, component specs) and `design-project/colors_and_type.css` (canonical token block — load before Tailwind). Direction: human-approachable, cool-gray neutrals, single green-teal OKLch-170 accent, freshness color scale, hairline borders, rounded components.
- **Assets:** optimized photography under `design-project/images/` and `design-project/assets/`.
- **Fidelity rule:** tokens, typography scale, spacing rhythm, radii, shadows, motion, and component states in `DESIGN.md`/`colors_and_type.css` are the visual contract. Where this file's prose and the exported design disagree on visual details, the exported design wins; where the design lacks a screen, follow the page specs below and reuse the same component system.
- **Styling implementation:** the design is implemented with **Tailwind CSS 4.3** (see `REQUIREMENTS.md` §12). The token block in `colors_and_type.css` maps to Tailwind theme tokens (`@theme`), not hand-written per-screen CSS.

The page specs use one consistent template so each page is described in the same shape.

## Design principles (from `REQUIREMENTS.md`)

- **Server-rendered.** HTML is produced by Askama. HTMX handles server-state interactions (search, filter, forms). Alpine.js is limited to *local UI state only* (map, dropdowns, modals, filter-panel toggles).
- **Responsive** across desktop, tablet, mobile with the same HTML.
- **WCAG 2.2 AA:** semantic HTML, keyboard operability, visible focus, labeled forms, sufficient contrast, accessible dialogs, and always a **non-map** representation of results.
- **i18n:** Portuguese (Brazil) and English; user-facing strings are not hard-coded in domain/application logic.
- **Server is the source of truth** for business/server state.

## Global layout (`layouts/base.html`)

- **Header:** logo → home; compact search box; primary nav (`Login`/`Register` or account menu); language switcher (pt-BR / en).
- **Flash region:** HTMX success/error notifications (dismissible).
- **Footer:** About, Privacy, Terms, Cookies, contact; language switcher.
- **Mobile:** collapsible nav; search remains reachable in one tap.

## Shared components

These are reused across pages; each has a spec in the same style as pages.

| Component | Purpose | State handling |
|---|---|---|
| **Search bar** | Address/place autocomplete + "use my location" | HTMX autocomplete; Alpine for the geolocation button state |
| **Map panel** | MapLibre map: destination, markers, current position; pan/zoom/recenter; marker↔card sync | Alpine (local map state); HTMX for loading details |
| **Parking card** | Name, distance, type badge, security badges, cost, rating, freshness badge, open-now | Server-rendered; HTMX for select/expand |
| **Filter & sort panel** | cost/type/security/radius filters + sort order | HTMX (updates results); Alpine (panel open/close); state in URL query params |
| **Rating display + review form** | 5-star read display; 5-star input | HTMX submit |
| **Freshness badge** | Fresh/Recently/Aging/Stale/Very stale | Server-rendered |
| **Security badges** | indoor/CCTV/staffed/… with explicit "unknown" | Server-rendered |
| **Image gallery** | Thumbnails → lightbox | Alpine (lightbox) |
| **Opening hours** | Weekly schedule in location timezone + "open now" | Server-rendered |
| **Empty state** | "No results near here — add the first parking location" | Server-rendered; HTMX CTA |
| **Modal** | Confirmations / quick actions | Alpine (open/close) |

---

# Page specifications

## Public (no authentication)

### P1 — Home / Landing
- **Purpose:** entry point; primary search.
- **Route:** `/`
- **Access:** public.
- **Sections:** hero with headline + large search bar; "use my location" button; brief "how it works" (3 steps); featured/example parking (optional, only once data exists); footer.
- **Data:** none required; may show a few example locations if available.
- **Interactions:** search submits → `/search?q=…`; geolocation button requests browser location then redirects to nearby search (discard location after use — §22).
- **States:** default; loading (search); geolocation denied (fallback to manual address entry, with a clear message).
- **Notes:** primary CTA must be reachable by keyboard; geolocation only after explicit user action.

### P2 — Search results
- **Purpose:** show nearby parking for a destination, filterable and sortable.
- **Route:** `/search?q=&lat=&lon=&radius=&type=&cost=&security=&sort=`
- **Access:** public.
- **Sections:** search bar (pre-filled, editable); map panel (left/desktop, toggleable on mobile); results list (cards); filter & sort panel; result count; pagination.
- **Data:** list of `ParkingLocation` summaries (name, distance, type, cost, security, rating, freshness, open-now), total count, current filters.
- **Interactions:** filters/sort → HTMX re-render results (URL updated for shareability); select card ↔ select marker (Alpine sync); card click → P3; pagination → HTMX (keyset); "recenter" returns map to destination/current position.
- **States:** loading (HTMX spinner); empty ("no results — add the first"); error (geocode failed / no destination); geolocation denied.
- **Notes:** the list is the accessible non-map alternative and must not depend on the map (§23/§63); results capped for map markers (§32).

### P3 — Parking details
- **Purpose:** everything a cyclist needs to decide where to park.
- **Route:** `/parking/{id}` (stable URL, §111).
- **Access:** public.
- **Sections:** name + type + moderation-visible state (if not ACTIVE, hidden from normal search); photo gallery; map (single location); key facts (cost, security, opening hours + open-now, freshness, rating, verification status); "recommended because…" explanation (§105); reviews list + summary; action bar (navigate externally, favorite, report); contributor actions if authenticated (propose change, review, verify, "I parked here", upload photo).
- **Data:** full `ParkingLocation`, photos (approved only), opening hours (location timezone), security features, reviews, verification signals, freshness.
- **Interactions:** external navigation link (§104); favorite toggle (HTMX); report (HTMX → modal); propose change (link → D2); review (→ D3); verify / "I parked here" (HTMX); gallery lightbox (Alpine).
- **States:** loading; not found (404); removed/invalid (only moderators/admins see); empty reviews; stale-information warning.
- **Notes:** opening hours shown in the location's timezone, with the timezone labeled (§29); verification results must not expose individual users' identities.

### P4 — Privacy policy
- **Purpose:** versioned privacy notice (§70).
- **Route:** `/privacy`
- **Access:** public.
- **Sections:** controller identity + contact, purposes, categories of data, legal bases, recipients/processors, international transfers, retention, data-subject rights + how to exercise, security practices, automated decision-making (if any), version + effective date.
- **Notes:** versioned so the system can tell which version a user saw; legal text is product/legal content, not assumed final text (§70).

### P5 — Terms of service
- **Purpose:** legal terms.
- **Route:** `/terms`
- **Access:** public.
- **Sections:** standard terms; version + effective date.

### P6 — Cookie policy
- **Purpose:** cookie disclosure (§78).
- **Route:** `/cookies`
- **Access:** public.
- **Sections:** strictly necessary vs optional cookies; purpose/lifetime/scope/security flags per cookie; note that no non-essential tracking cookies are used initially.

### P7 — About / How it works
- **Purpose:** explain the community model.
- **Route:** `/about` (optional, recommended).
- **Access:** public.
- **Sections:** mission; how to contribute; how verification/freshness works; moderation overview.

---

## Authentication

### A1 — Register
- **Purpose:** create an email/password account.
- **Route:** `/register`
- **Access:** public (redirect if already authenticated).
- **Sections:** email, password (+ confirmation), optional name; submit; "continue with Google"; link to login.
- **Interactions:** HTMX form submit with inline validation; OAuth button.
- **States:** validation errors (inline); email-in-use handled without revealing account existence (§45); success → "check your inbox" (email verification required).
- **Notes:** password strength guidance; rate-limited (§45).

### A2 — Login
- **Purpose:** authenticate.
- **Route:** `/login`
- **Access:** public.
- **Sections:** email, password, "forgot password?", submit; "continue with Google".
- **States:** invalid credentials (generic message, no account-existence leak); success → redirect to intended page; suspended/deleted accounts blocked.

### A3 — Email verification
- **Purpose:** verify a newly registered email.
- **Route:** `/verify-email?token=…`
- **Access:** public (token-gated).
- **Sections:** success confirmation, or invalid/expired token with "resend verification email".
- **Notes:** token single-use, expiring (§16); resend is rate-limited.

### A4 — Password reset request
- **Purpose:** request a reset link.
- **Route:** `/password-reset`
- **Access:** public.
- **Sections:** email field + submit.
- **Notes:** always show a neutral "if that address exists, a link has been sent" message (§45); rate-limited.

### A5 — Password reset (set new)
- **Purpose:** set a new password.
- **Route:** `/password-reset?token=…`
- **Access:** public (token-gated).
- **Sections:** new password + confirmation + submit.
- **States:** success → login; invalid/expired token.
- **Notes:** invalidates existing sessions where appropriate (§18).

### A6 — OAuth callback / account linking (flow, not a standalone page)
- **Purpose:** complete Google authentication and map to `AuthenticationIdentity`.
- **Route:** `/auth/google/callback` (internal).
- **Access:** public (redirect flow).
- **Sections:** no visible UI; shows a transient "signing in…" state, then redirects.
- **Notes:** Google identity maps to the same internal user model (§16); never exposes identity externally (§17).

---

## Account (authenticated)

### C1 — Account overview / settings
- **Purpose:** hub for account management.
- **Route:** `/account`
- **Access:** any authenticated user.
- **Sections:** profile summary (email, verification status, role if applicable); navigation to password/email/favorites/contributions/privacy.
- **States:** unverified banner ("verify your email to contribute").

### C2 — Change password
- **Route:** `/account/password`
- **Access:** authenticated.
- **Sections:** current password, new password, confirmation.
- **Notes:** invalidates other sessions where appropriate (§18); audit event (§47).

### C3 — Change email
- **Route:** `/account/email`
- **Access:** authenticated.
- **Sections:** new email, current password; verification of new email before it takes effect.
- **Notes:** audit event; do not leak OAuth identities.

### C4 — Favorites
- **Route:** `/account/favorites`
- **Access:** authenticated.
- **Sections:** list of favorited parking (parking cards), remove action.
- **Notes:** private, never publicly visible (§42).

### C5 — Contribution history
- **Route:** `/account/contributions`
- **Access:** authenticated.
- **Sections:** the user's own contributions: created locations, proposals, reviews, verifications, reports; status of each (pending/approved/rejected).
- **Notes:** attributes changes internally; public attribution uses anonymized/limited info (§46).

### C6 — Privacy & data
- **Route:** `/account/privacy`
- **Access:** authenticated.
- **Sections:** request data export; request account deletion; exercise other rights (rectification/restriction/objection/consent-withdrawal); view/consent records.
- **Notes:** requests auditable and identity-verified (§72); consent records withdrawable (§69).

### C7 — Export status / download
- **Route:** `/account/export/{id}`
- **Access:** authenticated (owner only).
- **Sections:** export status (processing/ready), download link when ready.
- **Notes:** link expires, authenticated, not indexable (§73).

---

## Contribution (authenticated, verified)

### D1 — Add parking location
- **Route:** `/parking/new`
- **Access:** authenticated + email-verified (§16).
- **Sections:** name, address, map pin (drop/confirm coordinates), type, cost (+ currency/amount/unit if paid), opening hours (structured, in location timezone), security features, description, photos (optional).
- **Interactions:** map pin placement (Alpine); address → coordinates (HTMX geocode); duplicate-detection warning (advisory, non-blocking — §36); timezone auto-derived from coordinates, confirmable/overridable (§29).
- **States:** duplicate warning; validation errors; submit success → details page.
- **Notes:** creator identity not auto-public (§35); rate-limited (§45).

### D2 — Edit / propose change
- **Route:** `/parking/{id}/edit`
- **Access:** authenticated + verified.
- **Sections:** editable fields as in D1, pre-filled; changes submitted as a proposal with history.
- **Notes:** important changes retain history rather than silently overwriting (§37); moderators may approve/reject (§37).

### D3 — Write / edit review
- **Route:** `/parking/{id}/review`
- **Access:** authenticated + verified.
- **Sections:** 5-star rating input, text review, optional photos.
- **Notes:** one active review per user per location, editable (§38); edits preserve audit/history; reviews have moderation states (§38).

### D4 — Verify / "I parked here" (modal on P3)
- **Purpose:** submit verification signals.
- **Route:** HTMX endpoints on `/parking/{id}`.
- **Access:** authenticated + verified.
- **Sections:** "still exists / no longer exists / information changed"; per-attribute verification; "I parked here".
- **Notes:** records user/timestamp/location/attribute/result without publicly exposing identity (§39); "I parked here" is private and short-retained (§41); rate-limited (§45).

---

## Moderation (moderator/admin)

### M1 — Moderation dashboard
- **Route:** `/moderation`
- **Access:** MODERATOR, ADMIN.
- **Sections:** overview counts + links to queues: photos, reports, proposals; recent activity.
- **Notes:** all moderation actions write audit events (§44).

### M2 — Photo moderation queue
- **Route:** `/moderation/photos`
- **Access:** MODERATOR, ADMIN.
- **Sections:** grid/list of `PENDING_REVIEW` photos with approve/reject actions; linked parking location.
- **Notes:** photos not public until approved (§30/§116.2).

### M3 — Reports queue
- **Route:** `/moderation/reports`
- **Access:** MODERATOR, ADMIN.
- **Sections:** list of reports by state (`OPEN`/`UNDER_REVIEW`/`RESOLVED`/`DISMISSED`), with target content and actions to resolve/dismiss.
- **Notes:** users cannot resolve their own reports (§43).

### M4 — Proposal review
- **Route:** `/moderation/proposals`
- **Access:** MODERATOR, ADMIN.
- **Sections:** list of proposed changes with diff/context; approve/reject/modify.
- **Notes:** important changes retain history (§37/§107).

### M5 — User management (admin)
- **Route:** `/admin/users`
- **Access:** ADMIN.
- **Sections:** user list/search; account state (active/suspended); role assignment (grant/revoke MODERATOR/ADMIN).
- **Notes:** role changes audited + deny-by-default (§19); suspend abusive users (§44).

### M6 — Audit log (admin)
- **Route:** `/admin/audit`
- **Access:** ADMIN.
- **Sections:** filterable audit events (actor, action, target, timestamp, result, metadata).
- **Notes:** audit records access-controlled; no secrets/PII in logs (§47/§86).

---

## Error / utility

### E1 — Not found (404)
- **Route:** fallback.
- **Access:** public.
- **Sections:** message + link home + search.

### E2 — Error (5xx)
- **Route:** `/error`.
- **Access:** public.
- **Sections:** generic message; no internal error details (§85).
- **Notes:** must be safe as an htmx-4 swap fragment (htmx 4 swaps 4xx/5xx by default — §116.6).

---

## Page coverage checklist (for the mockup/design phase)

Public: P1–P7. Auth: A1–A6. Account: C1–C7. Contribution: D1–D4. Moderation: M1–M6. Errors: E1–E2.

Design files should be produced at least for: **P1, P2, P3, A1, A2, C1, C4, C6, D1, D3, M1, M2, M3** (the highest-traffic / most-structure-critical pages), with the remainder following the same component system.
