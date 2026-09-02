# M5 — Moderation & reporting — implementation plan

> **Status: planned.** Derived from `PLAN.md` (M5) and `REQUIREMENTS.md`
> (§19–§20, §30, §37–§47, §80, §103). Parent plan: `PLAN.md`.

Companion to `REQUIREMENTS.md` (§43–§47 drive this milestone), `PLAN.md` (M5 overview) and
`UI_DESIGN.md` + `design-project/` screens `m1-moderation.html`, `m3-reports.html`,
`m4-proposals.html`, `m5-user-management.html`, `m6-audit-log.html`, plus the P3 "report" action
and the D3 review-photo attach — the visual contract.

**What already exists:** the `audit_events` table + `AuditLog` port (`record` only) and
`SqlxAuditLog` (M2); `review.moderation_state` (`ACTIVE`/`HIDDEN`, only `ACTIVE` is ever set and
read — M3); `parking_location.moderation_state` (`ACTIVE`/`PENDING_REVIEW`/`FLAGGED`/`INVALID`/
`REMOVED`, only `ACTIVE` is used; search filters `ACTIVE` — M1/M3); `parking_photo.moderation_state`
(`PENDING_REVIEW`/`APPROVED`/`REJECTED` — M4); `parking_proposal` (created `PENDING` in M3,
resolution deferred); `AuthService::grant_role`/`revoke_role` + `/admin/users` role assignment
(M2, audited); `ContributionHistoryReader` (self-history, C5). M5 is the **reports + moderation
actions + audit viewer** work on top of that foundation, and it **closes the deferred items** from
M3/M4 (proposal resolution, review moderation, photo hide, report-a-photo, review-photo attach).

**Goal:** a defensible moderation and audit layer over user-generated content. Users can report;
moderators triage reports, hide/restore content, invalidate parking, and resolve proposals; admins
suspend users and read the audit trail — every action written to the audit log, and no user able to
resolve their own report.

**Working app means (acceptance):** a user reports a review (it becomes `OPEN`, visible only to
moderators); a moderator claims it (`UNDER_REVIEW`) and resolves it, hiding the review (it
disappears from P3) — the audit trail shows who did what; a user's attempt to resolve their own
report is rejected; a photo can be reported and hidden; a location can be invalidated and restored;
a `PENDING` proposal can be approved (the change applies with a revision) or rejected; an admin
suspends an abusive user (sessions revoked, login blocked) and restores them; the audit viewer lists
filtered events. `cargo test` green; fresh-clone onboarding from README still works.

---

## 1. Scope

### In scope

| Area | Content |
|---|---|
| Schema | `0010_moderation.sql`: `report` table; `parking_photo` gains a `HIDDEN` moderation state (drop/re-add CHECK); audit-viewer indexes. `0011_review_photos.sql`: `review_photo` table (D3 attach, deferred from M4) |
| Domain | `ReportState`/`ReportTargetType`/`ReportReason` (code list + allowed-reason mapping); `PhotoModerationState::Hidden`; parking moderation transitions documented as an invariant |
| Application | `ReportRepository`/`ModerationRepository`/`AuditLogReader` ports; `ModerationService` use cases; `AuthService::suspend_user`/`restore_user`; generalize `PhotoService` over `PhotoTarget { Parking, Review }`; report rate-limit defaults |
| Infrastructure | `SqlxReportRepository`; `SqlxModerationRepository` (hide/restore/invalidate/proposal-apply); `SqlxAuditLogReader`; review-photo repository + generalized photo queue reader |
| Web | P3/review-card "report" modal; M1 dashboard; M3 reports queue (claim/resolve/dismiss); M4 proposals queue (approve/reject); hide/restore actions; `/admin/users` suspend/restore; `/admin/audit` viewer; `/admin/users/{id}/contributions`; D3 review-photo attach; i18n additions |
| Gating | `require_moderator` for moderation queues/actions; `require_role(Admin)` for suspend/restore + audit viewer; report submission gated to authenticated users |

### Explicitly out of scope (deferred, with where it lands)

| Item | Lands in |
|---|---|
| Real S3 / real `MEDIA_SIGNING_SECRET`; shared/Redis rate limiter; configurable upload+moderation constants | M7 (Ledger #6/#7/#14/#18) |
| Retention/purge of resolved reports, abandoned upload objects, expired audit rows | M6 retention jobs |
| "I parked here" purge job | M6 (M3 already sets `expires_at`) |
| Automatic face/license-plate detection (§80 "MAY be introduced later") | not required for initial release |
| Report **appeal** flow / re-open a resolved report | not required for initial release (§43 only mandates the four states) |
| i18n SEO (`hreflang`) and CSP hardening over the new moderation pages | M7 |
| Proposal "modify" as a fully separate UI state | realized here as **approve-with-adjusted-values** (see §2); a dedicated modify screen is not required |

---

## 2. Decisions

| Decision | Choice | Reasoning |
|---|---|---|
| **Report target model** | `report.target_type ∈ {parking, parking_photo, review, review_photo}` + `target_id BIGINT` (no FK — polymorphic across four tables). The service validates the target row exists on submit | FK can't span tables; an explicit four-way type keeps resolution unambiguous (vs. a single `photo` type that would collide across `parking_photo`/`review_photo`). The four types map to UI_DESIGN's "target content" |
| **Report reasons** | Hardcoded domain code list `REPORT_REASONS` (`nonexistent_parking`, `incorrect_location`, `incorrect_price`, `incorrect_hours`, `incorrect_security`, `duplicate`, `inappropriate_photo`, `inappropriate_review`, `spam`, `abuse`, `other`) + i18n labels. `reason_allowed_for(target_type, reason)` enforces a sensible mapping (e.g. `inappropriate_photo` only on photo targets) | §43 lists eleven reasons; mirroring the §28 security-attribute approach (code in domain, label via i18n, no DB catalog) keeps them localizable without a migration |
| **Report state machine** | `OPEN → UNDER_REVIEW → RESOLVED | DISMISSED` (both terminal). `claim` is the only `OPEN → UNDER_REVIEW` move; `resolve`/`dismiss` are the terminal moves. No re-open in this milestone | §43 mandates exactly these four states; a two-step claim→resolve flow records *who claimed* and *who resolved* separately (§47) |
| **Self-resolve guard** | Enforced in `ResolveReport`/`DismissReport` **server-side**: `reporter_id == moderator` → `SelfResolve` error. The UI additionally hides resolve controls on one's own report | §43 "Users MUST NOT be able to … resolve their own reports" — a server rule, not a UI affordance |
| **Photo hide state** | Add `HIDDEN` to `parking_photo.moderation_state` (and `review_photo`). `HIDDEN` **retains** the derivatives (restorable) and is distinct from `REJECTED` (bytes deleted — M4). `is_publicly_visible()` stays `matches!(Approved)` so hidden/approved/rejected are all non-public | §44 "hide … restore content where appropriate". Re-using `REJECTED` would delete the bytes M4 guarantees are gone on reject and would break restore. The existing reader already filters `APPROVED`, so `HIDDEN` is automatically non-public |
| **Parking invalidation vs removal** | Moderator `invalidate` → `INVALID` (takedown for quality/policy, restorable). Approving a `change_existence` proposal (existence = removed) → `REMOVED` (community-confirmed gone). Both non-public (`is_publicly_visible()` = `ACTIVE` only); both restorable by a moderator to `ACTIVE`. Each writes a `parking_revision` with `change_kind = 'moderation'` | The M1 enum already has both states; assigning distinct meanings keeps the audit trail honest about *why* a listing disappeared (§46 distinguishes moderation vs. removal). `FLAGGED`/`PENDING_REVIEW` remain unused this milestone |
| **Proposal resolution** | `approve` applies the proposal's change (move → update point+timezone; change_existence → set `REMOVED`/`ACTIVE`), bumps `version`, appends a `moderation` revision, sets status `APPROVED`, and **supersedes** older `PENDING` proposals on the same location. `reject` sets `REJECTED` + reason, no live change. **"modify"** = approve-with-adjusted-values: the approve form may carry corrected coordinates/timezone/existence, which are applied instead of the original `proposed` JSONB | §37/§107 (sensitive changes retain history), §44 "review proposed changes". Applying on approval is the whole point of the gated flow; recording the moderator's adjusted values keeps history faithful |
| **Review-photo attach (§38)** | New `review_photo` table reusing the M4 `ImageProcessor` seam + domain constants + `ObjectStorage` (same `PENDING_REVIEW → APPROVED/REJECTED/HIDDEN` lifecycle, same EXIF-strip/thumbnail/derivative-only policy). D3 review form becomes **multipart** (rating + body + 0..N photos). The review text publishes immediately (`ACTIVE`); its photos are held pending and only `APPROVED` ones render on the review card | §38 "optionally upload photos". Sharing the processor/constants avoids a second pipeline; the "text live, media moderated" split matches how location photos behave and is the §30/§116.2 moderation contract |
| **Photo queue generalization** | `PhotoService` is generalized over `PhotoTarget { Parking(location_id), Review(review_id) }`; `list_pending` returns tagged items and the M2 queue (`/moderation/photos`) renders both kinds in one oldest-first list. Approve/reject routes take a `kind` + `id` | One mental model for moderators ("photos need review") rather than two queues. This is a contained refactor of M4's just-landed path — M4 tests are the guard |
| **Suspend/restore** | ADMIN-only. `suspend` → `AccountState::Suspended` + **revoke all active sessions** + audit `user.suspended`; `restore` → `ACTIVE` + audit `user.restored`. `require_user`/`require_verified` gain a `can_access_account` check so a mid-session suspension takes effect immediately | §44 "suspend abusive users where authorized", §20 suspended users can't act. Account lifecycle + role changes are the most sensitive ops — ADMIN gate mirrors §19. Revoking sessions makes suspension immediate, not just "next login" |
| **Audit viewer** | New `AuditLogReader` port + `SqlxAuditLogReader` (filters: actor, action, target_type, date range; keyset pagination on `id DESC`). `/admin/audit` is ADMIN-only; it may resolve actor ids to emails (internal investigation), never to public pages | §47 "access controls + retention" + "no passwords/tokens/PII". Metadata is rendered as an escaped JSON blob; by construction audit writers put no secrets in `metadata` |
| **Contribution-history inspection** | Extend `ContributionHistoryReader` with a target-user variant; `/admin/users/{id}/contributions` (MODERATOR/ADMIN) reuses the C5 aggregation scoped to that user | §44 "inspect contribution history"; the M3 reader already aggregates across all five contribution tables — only the scope changes |
| **Report rate limiting (§45)** | Reuse the `RateLimiter` port (Ledger #6). Default: `report:create:user:{id}` **10/day/user** + `report:create:ip:{ip}` **20/day/IP** | §45 mandates report-creation limits; defaults chosen now, tuned in M7 |
| **Report description safety (§103)** | `description` is optional, trimmed, `0..=1000` chars; reason must be a known code; target must exist. Rendered escaped by Askama (never `|safe`) | §103 — report descriptions are untrusted UGC |
| **Existing gap closed** | The P3 details handler currently renders any `moderation_state`; M5 makes public P3 return **404** for non-`ACTIVE` locations, while moderators/admins still see the page with a "hidden/invalid/removed" banner | UI_DESIGN P3 states: "removed/invalid (only moderators/admins see)"; search already filters `ACTIVE`, the details page was the leak |
| **Compile-time SQL** | Continue `query_as!`/`query!` for all new readers/writers | §9/§305, established M1–M4 |

---

## 3. Schema

### `migrations/0010_moderation.sql`

```sql
-- §43/§44/§47: reports, photo hide state, audit-viewer indexes.

CREATE TABLE report (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    reporter_id     BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    target_type     TEXT NOT NULL CHECK (target_type IN
                        ('parking', 'parking_photo', 'review', 'review_photo')),
    target_id       BIGINT NOT NULL,        -- row id in the target table (no FK: polymorphic)
    reason          TEXT NOT NULL,          -- domain code from REPORT_REASONS (§43)
    description     TEXT,                   -- optional, <= 1000 chars (§103)
    state           TEXT NOT NULL DEFAULT 'OPEN'
                    CHECK (state IN ('OPEN', 'UNDER_REVIEW', 'RESOLVED', 'DISMISSED')),
    claimed_by      BIGINT REFERENCES users(id) ON DELETE SET NULL,
    resolved_by     BIGINT REFERENCES users(id) ON DELETE SET NULL,
    resolution_note TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX report_state_idx     ON report (state, created_at);
CREATE INDEX report_target_idx    ON report (target_type, target_id);
CREATE INDEX report_reporter_idx  ON report (reporter_id, created_at DESC);

-- Photo moderation gains HIDDEN (§44 hide/restore). Drop + re-add the CHECK
-- (the auto-generated name is <table>_<column>_check).
ALTER TABLE parking_photo DROP CONSTRAINT parking_photo_moderation_state_check;
ALTER TABLE parking_photo ADD CONSTRAINT parking_photo_moderation_state_check
    CHECK (moderation_state IN ('PENDING_REVIEW', 'APPROVED', 'REJECTED', 'HIDDEN'));

-- Audit viewer (§47): filter by target and time.
CREATE INDEX audit_events_target_idx  ON audit_events (target_type, target_id);
CREATE INDEX audit_events_created_idx ON audit_events (created_at DESC);
```

### `migrations/0011_review_photos.sql`

```sql
-- §38 review-photo attach (deferred from M4). Same storage/EXIF/thumbnail and
-- moderation contract as parking_photo (§30/§80): uploads are processed
-- derivatives only, held PENDING_REVIEW, APPROVED-only visible, HIDDEN
-- restorable.

CREATE TABLE review_photo (
    id               BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    review_id        BIGINT NOT NULL REFERENCES review(id) ON DELETE CASCADE,
    uploader_id      BIGINT REFERENCES users(id) ON DELETE SET NULL,
    storage_key      TEXT NOT NULL,
    thumbnail_key    TEXT,
    width            INTEGER,
    height           INTEGER,
    processed_at     TIMESTAMPTZ,
    moderation_state TEXT NOT NULL DEFAULT 'PENDING_REVIEW'
        CHECK (moderation_state IN ('PENDING_REVIEW', 'APPROVED', 'REJECTED', 'HIDDEN')),
    rejection_reason TEXT,
    reviewed_by      BIGINT REFERENCES users(id) ON DELETE SET NULL,
    reviewed_at      TIMESTAMPTZ,
    position         INTEGER NOT NULL DEFAULT 0,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX review_photo_pending_idx ON review_photo (moderation_state, created_at)
    WHERE moderation_state = 'PENDING_REVIEW';
CREATE INDEX review_photo_review_idx ON review_photo (review_id, position, id)
    WHERE moderation_state = 'APPROVED';
CREATE INDEX review_photo_uploader_idx ON review_photo (uploader_id, created_at DESC);
```

Notes:

- `seed-mock` keeps inserting `APPROVED` parking photos explicitly, so the widened CHECK doesn't
  affect it. Seeded rows have no `review_photo` counterpart (reviews aren't seeded — M3 decision).
- `report.target_id` is intentionally FK-less; `target_type` + service-side existence checks are the
  integrity guard. A later "purge orphaned reports" could be a retention job (M6), not this migration.

---

## 4. Domain model (crates/domain)

New module `crates/domain/src/moderation.rs` (pure, no I/O):

```
ReportState       { Open, UnderReview, Resolved, Dismissed }   // as_code/from_code
ReportTargetType  { Parking, ParkingPhoto, Review, ReviewPhoto } // as_code/from_code

pub const REPORT_REASONS: &[&str] = &[
    "nonexistent_parking", "incorrect_location", "incorrect_price", "incorrect_hours",
    "incorrect_security", "duplicate", "inappropriate_photo", "inappropriate_review",
    "spam", "abuse", "other",
];
pub fn is_known_report_reason(code: &str) -> bool { … }

/// Enforces §43's sensible mapping: which reasons may target which entity.
pub fn reason_allowed_for(target: ReportTargetType, reason: &str) -> bool { … }
```

`ReportDescription` value object (trimmed, `0..=1000` chars) — or a free `validate_report_description`
helper. Keep it as a small `ReportDescription(String)` mirroring `ReviewBody`.

`photo.rs` additions:

- `PhotoModerationState::Hidden` with `as_code() = "HIDDEN"` and a round-trip in `from_code`.
- `is_publicly_visible()` unchanged (`Approved` only) — `Hidden`/`Rejected`/`PendingReview` all false.

Parking moderation invariant (documented, enforced in the application service):

- `ACTIVE ↔ INVALID` (moderator invalidate/restore) and `ACTIVE → REMOVED` (approved removal
  proposal) / `REMOVED → ACTIVE` (restore). No transition from `INVALID`/`REMOVED` back to
  `PENDING_REVIEW`/`FLAGGED`.

Domain unit tests: `ReportState`/`ReportTargetType` round-trips; `REPORT_REASONS` all known;
`reason_allowed_for` boundaries (`inappropriate_photo` on photo targets only, `duplicate` on
parking only, `spam`/`abuse` on review/parking targets); `ReportDescription` length/trim boundaries;
`PhotoModerationState::Hidden` round-trip + `is_publicly_visible()` still false.

---

## 5. Application layer (crates/application)

New module `crates/application/src/moderation.rs`; `auth.rs` gains suspend/restore; `photo.rs` gains
the `PhotoTarget` generalization.

### Ports

```rust
// moderation.rs
pub struct NewReport {
    pub reporter_id: UserId,
    pub target_type: ReportTargetType,
    pub target_id: i64,
    pub reason: String,           // known code
    pub description: Option<ReportDescription>,
}

#[async_trait] trait ReportRepository: Send + Sync {
    async fn create(&self, r: &NewReport) -> Result<i64, ModerationError>;
    async fn list(&self, state: Option<ReportState>) -> Result<Vec<Report>, ModerationError>;
    async fn get(&self, id: i64) -> Result<Option<Report>, ModerationError>;   // includes reporter_id
    async fn claim(&self, id: i64, moderator: UserId) -> Result<(), ModerationError>;   // OPEN → UNDER_REVIEW
    async fn resolve(&self, id: i64, moderator: UserId, note: &str, outcome: ReportOutcome) -> Result<(), ModerationError>;
}

// The target-existence check is a repo responsibility (one lookup per target table).
#[async_trait] trait ModerationRepository: Send + Sync {
    async fn target_exists(&self, target_type: ReportTargetType, target_id: i64) -> Result<bool, ModerationError>;
    async fn hide_review(&self, id: i64, moderator: UserId) -> Result<(), ModerationError>;
    async fn restore_review(&self, id: i64, moderator: UserId) -> Result<(), ModerationError>;
    async fn hide_photo(&self, target: PhotoTarget, id: i64, moderator: UserId) -> Result<(), ModerationError>;
    async fn restore_photo(&self, target: PhotoTarget, id: i64, moderator: UserId) -> Result<(), ModerationError>;
    async fn set_parking_state(&self, id: i64, state: ModerationState, moderator: UserId) -> Result<(), ModerationError>;
    async fn list_pending_proposals(&self) -> Result<Vec<Proposal>, ModerationError>;
    /// Apply the proposal's change (or the moderator's adjusted values) + bump
    /// version + append a 'moderation' revision + set status APPROVED + supersede
    /// older PENDING proposals on the same location — one transaction.
    async fn approve_proposal(&self, id: i64, moderator: UserId, applied: ProposalApplication) -> Result<(), ModerationError>;
    async fn reject_proposal(&self, id: i64, moderator: UserId, reason: &str) -> Result<(), ModerationError>;
}

// audit.rs
#[async_trait] trait AuditLogReader: Send + Sync {
    async fn list(&self, filter: AuditFilter) -> Result<AuditPage, AuditError>;
}
```

`ReportOutcome { Resolved, Dismissed }`; `ProposalApplication { kind, proposed }` (the values to
apply — normally the proposal's own `proposed`, overridable by the approve form for "modify").
`AuditFilter { actor_id, action, target_type, from, to, cursor }` + keyset page.

`photo.rs` generalization: `enum PhotoTarget { Parking(i64), Review(i64) }`; `PhotoService::upload`
takes a `PhotoTarget` instead of a `location_id`; `approve`/`reject`/`list_pending` carry the target
kind. `PendingPhoto`/`PhotoForModeration` gain a `kind` field. The `PhotoRepository` port is split
into parking/review halves (or generalized with a `PhotoTarget` param) — implementation note, the
`ImageProcessor` seam is untouched.

### Use cases (`ModerationService`)

| Use case | Flow (abridged) |
|---|---|
| `SubmitReport` | authenticated → validate reason + target_type/reason mapping + description → rate-limit (`report:create:user:{id}` + `report:create:ip:{ip}`) → `target_exists` (else `TargetNotFound`) → `create` (state `OPEN`) → audit `report.created` |
| `ClaimReport` | `require_moderator` → `claim` (must be `OPEN`) → audit `report.claimed` |
| `ResolveReport` | `require_moderator` → `get` → **`reporter_id == moderator` → `SelfResolve`** → `resolve(outcome, note)` → audit `report.resolved`/`report.dismissed` |
| `HideReview` / `RestoreReview` | `require_moderator` → flip `ACTIVE ↔ HIDDEN` (idempotent-safe) → audit `review.hidden`/`review.restored` |
| `HidePhoto` / `RestorePhoto` | `require_moderator` → flip `APPROVED ↔ HIDDEN` for `parking_photo` or `review_photo` → audit `photo.hidden`/`photo.restored` |
| `InvalidateParking` / `RestoreParking` | `require_moderator` → `ACTIVE ↔ INVALID` + revision `change_kind=moderation` → audit `parking.invalidated`/`parking.restored` |
| `ListPendingProposals` | `require_moderator` → reader (web layer renders diff/context) |
| `ApproveProposal` | `require_moderator` → `approve_proposal(applied)` (move → update point+tz; removal → `REMOVED`; bump version; revision; supersede older PENDING) → audit `proposal.approved` |
| `RejectProposal` | `require_moderator` → `reject_proposal(reason)` → audit `proposal.rejected` |
| `SuspendUser` / `RestoreUser` | `require_role(Admin)` → `set_state(Suspended/Active)` + revoke sessions → audit `user.suspended`/`user.restored` |
| `ListAuditEvents` | `require_role(Admin)` → reader with filter |
| `ModerationDashboard` | `require_moderator` → counts: pending photos, pending review-photos, OPEN reports, UNDER_REVIEW reports, PENDING proposals + recent audit events |
| `UserContributionHistory` | `require_moderator` → `ContributionHistoryReader::for_user(target)` |

`ModerationError` variants: `NotAuthorized`, `SelfResolve`, `NotFound`, `TargetNotFound`,
`InvalidState` (wrong transition), `InvalidReason`, `RateLimited`, `Internal`. Mapped by the web
layer to friendly, non-leaking messages.

Report rate-limit defaults: `report:create:user:{id}` 10/day; `report:create:ip:{ip}` 20/day.
Moderation actions (hide/invalidate/approve/reject) are not rate-limited (moderators, audited).

---

## 6. Infrastructure (crates/infrastructure)

- `moderation/report.rs` — `SqlxReportRepository` (`query_as!`): `create`, `list` (optional state
  filter, ordered `created_at`), `get`, `claim` (`UPDATE … WHERE state='OPEN'` → `UNDER_REVIEW`,
  set `claimed_by`/`updated_at`), `resolve` (`UPDATE … WHERE state='UNDER_REVIEW'` → terminal, set
  `resolved_by`/`resolution_note`/`updated_at`). 0 rows affected → `InvalidState`.
- `moderation/actions.rs` — `SqlxModerationRepository`:
  `target_exists` (one lookup per target table); `hide_review`/`restore_review`
  (`UPDATE review SET moderation_state=…`); `hide_photo`/`restore_photo` (parking vs review table);
  `set_parking_state` (bump `version`, append `parking_revision` `change_kind='moderation'`,
  set state — one tx); `list_pending_proposals`; `approve_proposal` (apply point/tz or existence,
  bump version + revision, status `APPROVED`, and `UPDATE parking_proposal SET status='SUPERSEDED'
  WHERE location_id=… AND status='PENDING' AND id <> …` — one tx); `reject_proposal`.
- `moderation/audit.rs` — `SqlxAuditLogReader` (`query_as!`) with dynamic-ish filters (compose
  `WHERE` on the presence of each filter; keyset on `id DESC`). Metadata returned as `serde_json::Value`.
- `photo/repository.rs` — generalize to `PhotoTarget` (or add `review_photo` methods sharing the
  SQL shape): `insert_pending`, `approve`/`reject`/`hide`/`restore`, `list_pending` (UNION ALL over
  `parking_photo` + `review_photo` with a `kind` tag, ordered `created_at`), `max_position`.
- `community/history.rs` — add `for_user(target: UserId)` variant of the C5 aggregation.
- `auth/account_repo.rs` — `set_state` already exists (M2); add `revoke_all_sessions(user)` for
  suspend (update `sessions SET revoked_at=now() WHERE user_id=…`).

`test-support` additions: `ReportBuilder`, `ProposalBuilder` (PENDING), `ReviewPhotoBuilder`, and a
`ModeratorBuilder` (user with the MODERATOR role). Reuse the transaction/SAVEPOINT/committed-fixture
harness; read-model tests that hit other connections use the committed-fixture pattern (M1).

---

## 7. Web layer (crates/web)

### Middleware / gates

- `require_moderator` exists (M4); `require_role(Role::Admin)` exists (M2). `require_user`/
  `require_verified` gain the `can_access_account` check so suspended users are blocked mid-session.
- Report submission is gated to **authenticated** users (not verified — reporting abuse must work
  even for a brand-new account, but never anonymously; §43 says "Users").
- CSRF on every new POST (hidden input / `X-CSRF-Token`, same as M2–M4); the report modal is an
  HTMX `hx-post` fragment. htmx-4 4xx-swap-safe fragments on error (§116.6).

### Routes

| Route | Method | Page/action | Access |
|---|---|---|---|
| `/reports` | POST | submit a report (form: `target_type`, `target_id`, `reason`, `description`) | authenticated |
| `/moderation` | GET | M1 moderation dashboard | MODERATOR, ADMIN |
| `/moderation/reports` | GET | M3 reports queue | MODERATOR, ADMIN |
| `/moderation/reports/{id}/claim` | POST | claim (`OPEN → UNDER_REVIEW`) | MODERATOR, ADMIN |
| `/moderation/reports/{id}/resolve` | POST | resolve with note (HTMX) | MODERATOR, ADMIN |
| `/moderation/reports/{id}/dismiss` | POST | dismiss with note (HTMX) | MODERATOR, ADMIN |
| `/moderation/proposals` | GET | M4 proposal review queue | MODERATOR, ADMIN |
| `/moderation/proposals/{id}/approve` | POST | approve (optionally adjusted values) | MODERATOR, ADMIN |
| `/moderation/proposals/{id}/reject` | POST | reject with reason | MODERATOR, ADMIN |
| `/moderation/reviews/{id}/hide` | POST | hide a review | MODERATOR, ADMIN |
| `/moderation/reviews/{id}/restore` | POST | restore a hidden review | MODERATOR, ADMIN |
| `/moderation/photos/{kind}/{id}/hide` | POST | hide an approved photo (`kind=parking|review`) | MODERATOR, ADMIN |
| `/moderation/photos/{kind}/{id}/restore` | POST | restore a hidden photo | MODERATOR, ADMIN |
| `/moderation/parking/{id}/invalidate` | POST | invalidate a location | MODERATOR, ADMIN |
| `/moderation/parking/{id}/restore` | POST | restore an invalid/removed location | MODERATOR, ADMIN |
| `/moderation/photos` (exists) | GET | M2 photo queue, now **both** photo kinds | MODERATOR, ADMIN |
| `/moderation/photos/{kind}/{id}/approve` / `/reject` (extend) | POST | approve/reject pending photo | MODERATOR, ADMIN |
| `/admin/users/{id}/suspend` | POST | suspend (revokes sessions) | ADMIN |
| `/admin/users/{id}/restore` | POST | restore to ACTIVE | ADMIN |
| `/admin/users/{id}/contributions` | GET | inspect a user's contribution history | MODERATOR, ADMIN |
| `/admin/audit` | GET | M6 audit-log viewer (filtered) | ADMIN |

- `/parking/{id}` (P3): **public gets 404 for non-`ACTIVE`**; moderators see the page + a
  "hidden/invalid/removed" banner + moderation actions. P3 gains the "Report" action (opens the
  report modal, target pre-filled `parking`), and each photo in the lightbox gains a
  "report this photo" action (`parking_photo`).
- `/parking/{id}/review` (D3): form becomes **multipart** (rating + body + 0..N `photo` files);
  `POST` runs the same `PhotoService::upload` per attached file with `PhotoTarget::Review(review_id)`.
  Review text publishes `ACTIVE` immediately; photos hold `PENDING_REVIEW`.

### Templates / i18n

- New pages: `pages/moderation_dashboard.html` (M1), `pages/moderation_reports.html` (M3),
  `pages/moderation_proposals.html` (M4), `pages/admin_audit.html` (M6); `pages/admin_users.html`
  gains suspend/restore buttons + a link to `/admin/users/{id}/contributions`.
- New partials: `report_form` (modal), `report_result`, `report_queue_item`, `proposal_queue_item`
  (diff/context + approve-with-adjusted form + reject), `audit_row`, `photo_queue_item`
  (generalized with a kind tag), `moderation_action_result` (swap-safe toasts).
- P3 additions: report action + report modal; photo report button; "this listing is hidden" banner
  for moderators. Review card (`review_card`) additions: approved photo thumbnails + lightbox +
  report-this-review action; D3 attach photos.
- **i18n additions** (`crates/web/src/i18n.rs`): full en/pt-BR for the 11 report reasons, the 4
  report states, target-type labels, all moderation action labels + toasts, proposal approve/reject/
  adjusted-values labels, suspend/restore + confirmation, audit filter labels + action names,
  "hidden/invalid/removed" banners, and all validation/error messages. Strings stay in the web
  catalog (§12/§102).
- The design screens `m1`, `m3`, `m4`, `m5`, `m6` are the visual contract; Tailwind utilities against
  the M0 `@theme` tokens, matching M1–M4. No inline `<script>` (Ledger #15).

---

## 8. Seeder / commands

No new commands. `seed-mock` keeps working (its photos pass `APPROVED` explicitly; the widened CHECK
doesn't affect it). No seeded reports/reviews — M5 demo data is created through the real forms, per
the M3 decision. `seed-admin` unchanged; the seeded admin already has the `ADMIN` role for
suspend/audit access.

---

## 9. Testing

| Layer | Tests |
|---|---|
| domain | `ReportState`/`ReportTargetType` round-trips; `REPORT_REASONS` + `reason_allowed_for` boundaries; `ReportDescription` length/trim; `PhotoModerationState::Hidden` round-trip + non-public |
| application | `SubmitReport` (auth gate, invalid reason/mapping, missing target → `TargetNotFound`, rate-limit, happy path → `OPEN` + audit); `ClaimReport`/`ResolveReport` (wrong-state → `InvalidState`, **self-resolve → `SelfResolve`**, audit); hide/restore review/photo/parking (idempotent, audit); `ApproveProposal` (move applies point+tz+revision, removal → `REMOVED`+revision, supersede older PENDING, audit) / `RejectProposal` (no live change); `SuspendUser`/`RestoreUser` (admin gate, session revoke, audit); `ListAuditEvents` filter; with fakes |
| infrastructure (`#[db_test]`) | report CRUD + state transitions (0-rows on wrong state); `parking_photo` CHECK accepts `HIDDEN` and rejects unknown; `review_photo` insert/approve/reject/hide; proposal approve applies change + writes revision + supersedes (concurrent PENDING proposals); audit reader filters + keyset pagination; combined photo queue UNION ordering (oldest first) |
| web (`#[db_test]`) | reporter cannot resolve own report (error, not just hidden UI); anonymous report → redirect; moderator claim → resolve → the hidden review **disappears from P3** and the audit row exists; hide photo → gone from gallery; invalidate parking → public P3/search 404/hidden, moderator still sees banner; suspend → login blocked + mid-session gate + sessions revoked; restore → login works; audit page + suspend + contribution-inspect all 403 for non-admins; report modal CSRF; rate-limit → 429; D3 multipart attach → review text `ACTIVE`, photos `PENDING_REVIEW`, approved-only render |
| security (§60/§61) | every new route gated (no moderator/admin action reachable without the role); reporter identity never rendered on public pages; audit viewer ADMIN-only and its metadata carries no secrets/PII; report description length/escaping (§103); suspended user's contributions blocked; moderation transitions each write an audit event; HTML-absence assertion for reporter/uploader emails on public pages |

---

## 10. Task breakdown

1. `0010_moderation.sql` + `0011_review_photos.sql`; verify `cargo run` applies them; confirm
   `seed-mock` still seeds `APPROVED` photos under the widened CHECK.
2. Domain: `moderation.rs` (state/target/reason + allowed-reason mapping + `ReportDescription`) +
   `PhotoModerationState::Hidden`; unit tests.
3. Application: `moderation.rs` (ports + `ModerationService` + report rate-limit + `ModerationError`);
   `auth.rs` `suspend_user`/`restore_user` (session revoke); generalize `photo.rs` over
   `PhotoTarget`; `AuditLogReader` port + `AuditFilter`; tests with fakes.
4. Infrastructure: `SqlxReportRepository`, `SqlxModerationRepository`, `SqlxAuditLogReader`,
   review-photo + generalized photo queue repo, `for_user` history, `revoke_all_sessions`;
   `test-support` builders; `#[db_test]` integration tests.
5. Web: routes + gates; `require_user`/`require_verified` `can_access_account` check; P3 404-for-
   non-ACTIVE + report modal; D3 multipart + review-photo attach; moderation dashboard/queues;
   hide/restore; suspend/restore; audit viewer; contribution inspect; templates/partials; i18n;
   Tailwind matching the design screens.
6. HTTP/security tests; README (new routes, moderation flow, suspend/audit notes); Ledger entries;
   live acceptance walkthrough against `docker compose` with a reporter user, a seeded moderator
   and the seeded admin.

## 11. Risks / notes

- **M4 `PhotoService` generalization** is the riskiest code change — refactoring the just-landed
  parking path to `PhotoTarget`. Keep M4's tests green throughout; the processor/constants are
  untouched, only the repository dispatch changes.
- **CHECK drop/re-add** — `parking_photo_moderation_state_check` is the auto-generated name; the
  migration is forward-only and applied once (standard SQLx). No backfill needed (`HIDDEN` is
  additive).
- **Self-resolve guard** must be server-side in the service, not just hidden in the template; test it
  explicitly with a reporter == moderator scenario.
- **Mid-session suspension** — `require_user`/`require_verified` must consult `account_state`, or a
  suspended user keeps contributing on an existing session. Revoking sessions + the gate check both
  apply.
- **Polymorphic `report.target_id`** has no FK; the `target_exists` check is the integrity guard.
  A stale/deleted target → `TargetNotFound` on submit (never a crash), and the report queue renders
  a "target removed" placeholder for orphaned rows.
- **Proposal apply correctness** — approval reads the *current* location row (not `base_version`
  snapshot) and bumps its `version`; `base_version` is recorded but does not block approval (the
  moderator is authoritative). Each apply writes a `moderation` revision so history stays faithful.
- **Audit metadata** — keep `metadata` free of secrets/PII; the viewer is admin-only and renders it
  as an escaped blob. Add an assertion that no password/token key ever reaches `metadata`.
- **htmx 4 4xx-swap** — every new POST must return swap-safe fragments on error (§116.6), reusing
  the M2–M4 error-partial approach.
- **CSP** — the report modal and moderation pages need no inline `<script>`; keep Alpine expressions
  out of server-rendered report/proposal data (Ledger #15).
- **Identity leaks** — reporter/uploader/proposer identities never render on public pages; the
  moderator/admin views may resolve them (internal) but never leak to `/parking/*` or search.

---

## Ledger additions this milestone

| # | Item | Kind | Introduced | Remove/improve by | Notes |
|---|---|---|---|---|---|
| 19 | Moderation constants hardcoded (report description length, report rate-limit defaults 10/day/user + 20/day/IP, proposal/adjust values) | improve | M5 | M7 | Make configurable + document for tuning, like Ledger #18 |

Also updated: **Ledger #6** now covers report-creation limits (10/day/user + 20/day/IP) in addition
to the M2 auth, M3 contribution, and M4 photo-upload limits.
