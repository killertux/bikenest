# M3 — Community contributions — implementation plan

> **Status: planned.** Derived from `PLAN.md` (M3) and `REQUIREMENTS.md`
> (§24–§25, §29, §34–§42, §45–§47, §100, §103, §105–§107). Parent plan: `PLAN.md`.

Companion to `REQUIREMENTS.md` (§35–§42 drive this milestone), `PLAN.md` (M3 overview) and
`UI_DESIGN.md` + `design-project/` screens `d1-add.html`, `d2-edit.html`, `d3-review.html`,
`c4-favorites.html`, `c5-contributions.html`, and the P3 additions (review list, verification
panel, favorite action, "recommended because…") — the visual contract.

**Goal:** verified users can grow and correct the dataset. Add locations, propose edits (with
field-level history), review, verify, and favorite — all persisted to real Postgres and visible,
with conflicting community signals surfaced rather than averaged away.

**Working app means (acceptance):** a verified user adds a location (timezone auto-derived from the
pin, overridable; duplicate warning shown), proposes an edit (reversible fields apply immediately
and a revision is retained; moving the pin / proposing removal queues a pending proposal), reviews
it (rating aggregate updates), verifies it (a conflicting "no longer exists" signal shows as
`Conflicting`, never averaged), sees why it's recommended, and favorites it — all persisted and
visible. `cargo test` green; fresh-clone onboarding from README still works.

---

## 1. Scope

### In scope

| Area | Content |
|---|---|
| Schema | `0007_contributions.sql`: `parking_location` gains `version` + `creator_id`; new `parking_revision`, `parking_proposal`. `0008_community.sql`: `review`, `review_revision`, `verification`, `favorite` |
| Domain | `StarRating`, `ReviewBody`, `VerificationKind`/`ExistenceResult`/`AttributeResult`, `Confidence` + pure resolution rule, `ProposalKind`/`ProposalStatus`, revision `ChangeKind`; `ParkingLocation` gains `version` |
| Application | `TimezoneResolver` port; contribution ports (`ParkingContributionRepository`, `ReviewRepository`, `VerificationRepository`, `FavoriteRepository`); `ContributionService` use cases; contribution rate-limit defaults; recommendation **explanation** (§105) |
| Infrastructure | `OfflineTimezoneResolver`; `SqlxParkingContributionRepository`, `SqlxReviewRepository`, `SqlxVerificationRepository`, `SqlxFavoriteRepository` (compile-time `query_as!`) |
| Web | Routes for D1 (`/parking/new`), D2 (`/parking/{id}/edit`), proposal (`/parking/{id}/proposal`), D3 (`/parking/{id}/review`), D4 (verify / parked-here endpoints), favorite toggle, C4 (`/account/favorites`), C5 (`/account/contributions`); P3 additions; `require_verified` gate; HTMX interactions; i18n additions |
| Commands | none new (`seed-mock`/`seed-admin` unchanged) |

### Explicitly out of scope (deferred, with where it lands)

| Item | Lands in |
|---|---|
| Photo upload / attachment to D1/D3 | M4 (the `parking_photo` pipeline; reviews are text+rating only in M3) |
| Proposal **review** (approve/reject/modify) UI | M5 (M4 page `moderation/proposals`); M3 only *creates* `PENDING` proposals |
| Moderation of reviews/locations (hide, invalidate) + reports | M5 (reports table + report form are M5; `review.moderation_state` is defined now, only `ACTIVE` is set) |
| "I parked here" purge job (retention enforcement) | M6 (M3 sets `expires_at`; the retention *policy* is defined here) |
| Real geocoder reverse-timezone | M7 (the offline resolver stays as fallback — Ledger #16) |
| Shared/Redis-backed rate limiter | M7 (Ledger #6, already introduced in M2) |
| Creator identity public attribution | never (§35/§46 — stored for internal attribution only) |

---

## 2. Decisions

| Decision | Choice | Reasoning |
|---|---|---|
| **Edit model** | **Hybrid.** Reversible fields (`name`, `address`, `description`, `parking_type`, `cost`, `opening_hours`, `security`) apply **immediately** in a transaction that bumps `version`, sets `last_meaningful_update_at`, and appends an immutable `parking_revision` (before-state snapshot). Sensitive changes (**location/coordinate move**, **existence/removal**) become `PENDING` `parking_proposal`s reviewed in M5 | §37 "retain history rather than silently overwriting" + §107; §100 for concurrency. Low-risk metadata edits should be instantly useful ("grow and correct the dataset"); moving a pin or deleting a listing can immediately mislead other cyclists and must be gated — this also gives M5's proposal-review queue a real object |
| **New location state** | **`ACTIVE`** — immediately searchable (revisions the M1 state-machine note that said `PENDING_REVIEW`) | PLAN M3 "persisted and visible"; §25 only says search returns `ACTIVE`, not that new rows must be non-ACTIVE. Guards are advisory duplicate detection (§36) + creation rate limiting (§45); takedown/reporting is M5. The enum already supports `PENDING_REVIEW` if pre-moderation is ever wanted |
| **Field-level history (§107)** | One `parking_revision` row per **applied** change, storing a JSONB **after-state snapshot** of the tracked fields (existence, location, type, cost, opening hours, security, moderation state) + `editor_id`, `version`, `change_kind`, `summary`. `UNIQUE(location_id, version)`; version 1 = creation | Storing full after-state (not a diff) makes reconstruction a pure read (state at version K = that row's snapshot) with no diff-replay logic. Tracked fields match the §107 minimum exactly |
| **Optimistic concurrency (§100)** | `parking_location.version BIGINT NOT NULL DEFAULT 1`. Every edit/apply reads `version`, and the `UPDATE … WHERE id = $ AND version = $expected` bumps it; 0 rows → `VersionConflict` → re-render the form with the latest values + a "someone else changed this" notice | §100 explicitly names parking edits; a version column is the simplest correct guard, and it feeds the proposal's `base_version` |
| **Proposal shape** | `parking_proposal(location_id, proposer_id, base_version, kind ∈ {move_location, change_existence}, proposed JSONB, status ∈ {PENDING, APPROVED, REJECTED, SUPERSEDED})`. D2's reversible fields POST to `/parking/{id}/edit`; the pin-move and removal actions POST to `/parking/{id}/proposal` separately (atomic, no mixed submits) | Keeping the two actions separate avoids "half direct, half gated" submissions. Approval logic (apply + revision + status flip) is M5 |
| **Duplicate detection (§36)** | Advisory, non-blocking. On D1 submit: query candidates within `DUPLICATE_RADIUS_M` (500 m) of the pin, then rank by normalized-name similarity (case-folded, diacritic-folded, Jaro–Winkler or trigram) + address token overlap. If any candidate scores above a threshold, return a **warning banner** listing them (with links) and ask the user to confirm; the user may still proceed | §36: advisory, "MUST NOT automatically delete or merge". Thresholds are constants in the application layer, tuned in M7 |
| **Timezone resolver (§29)** | `TimezoneResolver` port (`resolve(GeoPoint) -> Result<chrono_tz::Tz, …>`); `OfflineTimezoneResolver` implements it with a timezonefinder-style polygon dataset (Rust port, e.g. `tzf-rs` — confirm crate choice + data size at implementation). D1 shows the derived IANA tz pre-filled and **confirmable/overridable** | Replaces M1's static Curitiba mapping now that contributors supply arbitrary coordinates (§29/PLAN). Offline keeps the "no network in the hot path" rule; a real geocoder reverse-timezone can replace it in M7 (Ledger #16) |
| **Reviews (§38)** | `review` rows, `UNIQUE(location_id, author_id)` = one review per user per location (edits update in place). `rating SMALLINT 1..5`, `body 1..2000`, `moderation_state ∈ {ACTIVE, HIDDEN}`. Edits append to `review_revision` (prior `rating`/`body` + `edited_at`) so history is preserved | §38 "one active review per user … with the ability to edit it" + "edits MUST preserve audit/history". In-place edit + a revision chain is the lightest faithful model; `HIDDEN` is enforced by the reader (only `ACTIVE` is public) and set by M5 |
| **Rating aggregate** | `parking_location.rating_avg`/`rating_count` recomputed **in the same transaction** as any review create/edit from the `ACTIVE` reviews (`COUNT(*)` / `AVG(rating)`); no trigger | M1 documented these columns as "maintained by review use cases in M3". A recompute-in-transaction avoids denormalization drift; a test asserts it matches a direct `COUNT/AVG` |
| **Verification signals (§39)** | `verification` rows with `kind ∈ {existence, attribute, parked_here}`, `result`, optional `attribute_code`, `created_at`, and `expires_at` (only `parked_here`, = `now + 90 days`). No identity is ever rendered from them | §39 "record user/timestamp/location/attribute/result" + "identity MUST NOT be publicly exposed"; §41 parked-here is private + short-lived. Multiple signals over time are allowed; aggregation uses the latest per user |
| **Confidence (§106)** | `Confidence ∈ {Reported, Verified, RecentlyVerified, Stale, Conflicting}`, computed **per-detail-read** (not denormalized) from the latest existence signal per user. Rule in §4. `Conflicting` is reserved for existence contradictions (`no_longer_exists` vs `still_exists`); field-level disputes (`info_changed`, per-attribute `incorrect`) surface as a `disputed` flag + per-attribute counts, never averaged away | §106 "conflicting signals SHOULD NOT simply be averaged away". Read-time computation is cheap for a single location and avoids a denormalization sync problem; revisit in M7 perf if needed |
| **`last_verified_at` update** | A `still_exists` existence signal sets `parking_location.last_verified_at = now()` (the freshness source). `no_longer_exists`/`info_changed`/`parked_here` do **not** | Keeps freshness meaningful: it reflects the last *positive* existence confirmation, not negative/disputed signals |
| **"I parked here" retention (§41)** | Retained **90 days** (`expires_at = now + 90d`), private, never listed publicly; not fed into the confidence enum (it signals usage, not existence). The purge job is M6 | §41 recommended default adopted verbatim; the plan records the policy as required |
| **Recommendation explanation (§105)** | Extend the M1 scorer to also return a `RecommendationBreakdown` of per-factor reasons (distance / security / rating / freshness / verification), each mapped to an i18n label. P3 renders "recommended because…" from the **positive** factors only: distance only when the request carries an origin (`?lat=&lon=`, `?from=`), the rest from the location's own data. Missing data → factor omitted, never a fabricated claim | §105 "MUST NOT claim certainty the data can't support". Shares the same sub-score logic as the numeric `recommendation_score` so sorting and explanation never disagree |
| **Favorites (§42)** | `favorite(user_id, location_id, created_at)` PK `(user_id, location_id)`; toggle + list. Gated on **authenticated** (login), not email-verification | §42 says "Authenticated users MUST be able to favorite" (the M2 out-of-scope parenthetical that grouped favorites under "verified" is simplified; the requirement is the source of truth). Favorites are private, idempotent, low-risk |
| **Contribution gate** | `require_verified` = session principal with `is_verified` (from M2). Applies to add/edit/proposal/review/verify. Favorites apply to any authenticated user | §16/§35/§38/§39 all say "authenticated, verified users"; the M2 `is_verified` flag is the gate |
| **Rate limiting (§45)** | Reuse the M2 `RateLimiter` port (Ledger #6). Defaults: parking-create 5/day/user + 10/day/IP; edit 15/h/user; proposal 5/h/user; review 10/h/user; verification 30/h/user; parked-here 20/h/user. Favorites not rate-limited (idempotent + private). Keys are per authenticated user (`contribution:{kind}:user:{id}`) and, where listed, per-IP | §45 mandates limits for parking/review/verification creation; concrete defaults chosen now, documented for M7 tuning. Report + photo limits are M5/M4 |
| **Compile-time SQL** | Continue `query_as!`/`query!` for all new readers/writers | §9/§305, established M1/M2 |
| **Input safety (§103)** | All user-supplied text (name, description, address, review body, proposal `reason`) is length-validated in the domain and **escaped by Askama on render** (never `|safe` for user content) | §103 treats UGC as untrusted; Askama's default auto-escaping is the enforcement point |

---

## 3. Schema

### `migrations/0007_contributions.sql`

```sql
-- §100 optimistic concurrency + §35 creator capture.
ALTER TABLE parking_location
    ADD COLUMN version     BIGINT NOT NULL DEFAULT 1,
    ADD COLUMN creator_id  BIGINT REFERENCES users(id) ON DELETE SET NULL;
-- creator_id is stored for internal attribution only; never rendered publicly (§35/§46).

-- Immutable field-level history of APPLIED changes (§107). One row per change;
-- version 1 = creation. `snapshot` is the AFTER-state of the tracked fields.
CREATE TABLE parking_revision (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    location_id  BIGINT NOT NULL REFERENCES parking_location(id) ON DELETE CASCADE,
    version      BIGINT NOT NULL,                 -- parking_location.version AFTER this change
    editor_id    BIGINT REFERENCES users(id) ON DELETE SET NULL,  -- NULL = system/seed
    change_kind  TEXT NOT NULL,                   -- 'create' | 'edit' | 'moderation' | 'verification'
    summary      TEXT,                            -- short human description for C5
    snapshot     JSONB NOT NULL,                  -- {name,address,type,cost,point,tz,hours,security,moderation_state}
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (location_id, version)
);
CREATE INDEX parking_revision_location_idx ON parking_revision (location_id, version DESC);
CREATE INDEX parking_revision_editor_idx   ON parking_revision (editor_id, created_at DESC);

-- Gated sensitive changes (§37/§107): location move, existence/removal.
-- Created in M3 (PENDING); approved/rejected/modified in M5.
CREATE TABLE parking_proposal (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    location_id  BIGINT NOT NULL REFERENCES parking_location(id) ON DELETE CASCADE,
    proposer_id  BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    base_version BIGINT NOT NULL,                 -- parking_location.version at proposal time (§100)
    kind         TEXT NOT NULL CHECK (kind IN ('move_location', 'change_existence')),
    proposed     JSONB NOT NULL,                  -- move_location: {point, timezone, reason}
                                                  -- change_existence: {existence, reason}
    status       TEXT NOT NULL DEFAULT 'PENDING'
                 CHECK (status IN ('PENDING', 'APPROVED', 'REJECTED', 'SUPERSEDED')),
    resolved_by  BIGINT REFERENCES users(id) ON DELETE SET NULL,
    resolved_at  TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX parking_proposal_location_idx ON parking_proposal (location_id, status);
CREATE INDEX parking_proposal_status_idx   ON parking_proposal (status, created_at);
```

### `migrations/0008_community.sql`

```sql
-- Five-star reviews (§38). One row per user per location; edits update in place
-- and append to review_revision (history preserved).
CREATE TABLE review (
    id                BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    location_id       BIGINT NOT NULL REFERENCES parking_location(id) ON DELETE CASCADE,
    author_id         BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    rating            SMALLINT NOT NULL CHECK (rating BETWEEN 1 AND 5),
    body              TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 2000),
    moderation_state  TEXT NOT NULL DEFAULT 'ACTIVE'
                      CHECK (moderation_state IN ('ACTIVE', 'HIDDEN')),  -- HIDDEN set in M5
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (location_id, author_id)
);
CREATE INDEX review_location_idx ON review (location_id, created_at DESC);
CREATE INDEX review_author_idx   ON review (author_id, created_at DESC);

-- Review edit history (§38): one row per published version (initial + each edit).
CREATE TABLE review_revision (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    review_id  BIGINT NOT NULL REFERENCES review(id) ON DELETE CASCADE,
    rating     SMALLINT NOT NULL,
    body       TEXT NOT NULL,
    edited_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX review_revision_review_idx ON review_revision (review_id, id);

-- Verification signals (§39/§41). Multiple over time; aggregation uses the
-- latest per user. `expires_at` is set only for parked_here (now + 90 days).
CREATE TABLE verification (
    id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    location_id    BIGINT NOT NULL REFERENCES parking_location(id) ON DELETE CASCADE,
    user_id        BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind           TEXT NOT NULL CHECK (kind IN ('existence', 'attribute', 'parked_here')),
    result         TEXT NOT NULL,   -- existence: 'still_exists'|'no_longer_exists'|'info_changed'
                                    -- attribute: 'correct'|'incorrect'
                                    -- parked_here: 'parked_here'
    attribute_code TEXT,            -- for kind='attribute' (§39 per-attribute): name/address/type/cost/hours/security/location
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at     TIMESTAMPTZ      -- parked_here only
);
CREATE INDEX verification_location_idx    ON verification (location_id, created_at DESC);
CREATE INDEX verification_user_idx        ON verification (user_id, created_at DESC);
CREATE INDEX verification_parked_expiry   ON verification (expires_at) WHERE kind = 'parked_here';

-- Favorites (§42): private, per user.
CREATE TABLE favorite (
    user_id     BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    location_id BIGINT NOT NULL REFERENCES parking_location(id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, location_id)
);
CREATE INDEX favorite_location_idx ON favorite (location_id);
```

---

## 4. Domain model (crates/domain)

New module `crates/domain/src/community.rs` (pure, no I/O):

```
StarRating(u8)                       // 1..=5, validated
ReviewBody(String)                   // trimmed, 1..=2000 chars
VerificationKind { Existence, Attribute, ParkedHere }     // as_code/from_code
ExistenceResult { StillExists, NoLongerExists, InfoChanged }
AttributeResult { Correct, Incorrect }
ProposalKind { MoveLocation, ChangeExistence }
ProposalStatus { Pending, Approved, Rejected, Superseded }
ChangeKind { Create, Edit, Moderation, Verification }
Confidence { Reported, Verified, RecentlyVerified, Stale, Conflicting }
```

`ParkingLocation` gains `version: i64` (and its `new()` takes it). `creator_id` stays in the
repository/read model, not the public aggregate — the web layer never renders it.

**Confidence resolution rule** (`confidence(signals, now, thresholds) -> Confidence`), where
`signals` is the *latest* existence-verification per user (already deduped by the repository):

1. No existence signals → `Reported`.
2. Any `no_longer_exists` → `Conflicting` (the DB says active; the community says gone — never
   silently averaged; a moderator resolves it in M5).
3. Otherwise (all positives are `still_exists`): classify the **latest** `still_exists` by
   `freshness::categorize` → `Fresh` ⇒ `RecentlyVerified`; `RecentlyVerified`/`Aging` ⇒ `Verified`;
   `Stale`/`VeryStale` ⇒ `Stale`.

`info_changed` (and per-attribute `incorrect`) do **not** change the enum; they feed a separate
`disputed: bool` + per-attribute dispute counts surfaced on P3 (§106 — disputes are shown, never
averaged). `parked_here` is excluded from confidence entirely (usage, not existence).

Domain unit tests: `StarRating`/`ReviewBody` boundaries; `VerificationKind`/`ExistenceResult`/
`AttributeResult`/`ProposalKind`/`ProposalStatus`/`ChangeKind` code round-trips; `confidence`
covering every branch (empty → Reported; only positives → Verified/RecentlyVerified/Stale by
freshness; any `no_longer_exists` → Conflicting regardless of positives; `info_changed` → still
Reported but `disputed`).

---

## 5. Application layer (crates/application)

New module `crates/application/src/community.rs` + a new `timezone.rs`.

### Ports

```rust
// timezone.rs
#[async_trait] trait TimezoneResolver: Send + Sync {
    async fn resolve(&self, point: GeoPoint) -> Result<chrono_tz::Tz, TimezoneError>;
}

// community.rs
#[async_trait] trait ParkingContributionRepository: Send + Sync {
    async fn create(&self, new: NewParkingLocation) -> Result<i64, ContributionError>;  // id; writes revision v1
    async fn get_for_edit(&self, id: i64) -> Result<Option<ParkingForEdit>, ContributionError>;  // current fields + version
    /// Atomic optimistic apply: UPDATE ... WHERE version = expected; 0 rows → VersionConflict.
    async fn apply_edit(&self, id: i64, expected_version: i64, edit: ParkingEdit, editor: UserId)
        -> Result<i64, ContributionError>;  // new version
    async fn create_proposal(&self, p: NewProposal) -> Result<i64, ContributionError>;
    async fn revision_history(&self, id: i64) -> Result<Vec<RevisionSummary>, ContributionError>;
    async fn duplicate_candidates(&self, point: GeoPoint, name: &str) -> Result<Vec<DuplicateCandidate>, ContributionError>;
}

#[async_trait] trait ReviewRepository: Send + Sync {
    async fn upsert_review(&self, location_id: i64, author: UserId, rating: StarRating, body: &ReviewBody)
        -> Result<(), ContributionError>;  // insert-or-update + review_revision + recompute rating, one tx
    async fn find_own(&self, location_id: i64, author: UserId) -> Result<Option<Review>, ContributionError>;
    async fn list_active(&self, location_id: i64) -> Result<Vec<Review>, ContributionError>;  // ACTIVE only
}

#[async_trait] trait VerificationRepository: Send + Sync {
    async fn record(&self, signal: NewVerification) -> Result<(), ContributionError>;
    async fn latest_existence_per_user(&self, location_id: i64) -> Result<Vec<ExistenceSignal>, ContributionError>;
    async fn attribute_summary(&self, location_id: i64) -> Result<Vec<AttributeSummary>, ContributionError>;  // per-code correct/incorrect counts
    async fn parked_here_count(&self, location_id: i64) -> Result<i64, ContributionError>;
    async fn mark_verified_at(&self, location_id: i64, at: DateTime<Utc>) -> Result<(), ContributionError>;
}

#[async_trait] trait FavoriteRepository: Send + Sync {
    async fn toggle(&self, user: UserId, location_id: i64) -> Result<bool, ContributionError>;  // returns now-favorited?
    async fn is_favorited(&self, user: UserId, location_id: i64) -> Result<bool, ContributionError>;
    async fn list(&self, user: UserId) -> Result<Vec<i64>, ContributionError>;  // location ids (web layer joins cards)
}
```

### Use cases (`ContributionService`)

| Use case | Flow (abridged) |
|---|---|
| `AddParkingLocation` | `require_verified` → rate-limit (parking-create) → validate fields (domain) → resolve timezone (auto; override if provided) → run `duplicate_candidates` (advisory) → `create` (ACTIVE, creator, version 1, revision v1, `last_meaningful_update_at = now`) → audit `parking.created`. Returns the id + duplicate warnings |
| `ApplyParkingEdit` | `require_verified` → rate-limit (edit) → load `get_for_edit` → if any *sensitive* field changed, reject with "use the proposal action" (the web layer already routes those separately) → `apply_edit(expected_version, …)`; `VersionConflict` → re-render with latest + notice → audit `parking.edited` |
| `ProposeLocationChange` | `require_verified` → rate-limit (proposal) → `create_proposal` (kind `move_location`/`change_existence`, `base_version`) → audit `parking.proposal_created`. No live change |
| `UpsertReview` | `require_verified` → rate-limit (review) → validate `StarRating`/`ReviewBody` → `upsert_review` (one-per-user constraint; appends `review_revision`; recomputes `rating_avg`/`rating_count` in-tx) → audit `review.created`/`review.edited` |
| `RecordVerification` | `require_verified` → rate-limit (verification/parked-here) → validate kind/result/attribute → `record`; if `existence`+`still_exists` → `mark_verified_at(now)` → audit `verification.recorded` |
| `ToggleFavorite` / `ListFavorites` | authenticated (any state that can log in) → `toggle`/`list` |
| `ContributionHistory` | for C5: aggregate across `parking_location.creator_id`, `parking_revision.editor_id`, `review.author_id`, `verification.user_id`, `parking_proposal.proposer_id` into one time-ordered list of `{kind, target, status, at}` |
| `GetParkingDetails` (extended) | now also returns `reviews`, `confidence` (+`disputed`, per-attribute summary), `parked_here_count`, `is_favorited`, own-review/own-verification state, and the `RecommendationExplanation` |

`RecommendationExplanation` (§105): a companion to the existing `recommendation_score` in
`search.rs` — same sub-scores, but returned as `Vec<Reason>` (each `{ factor, label_key, detail }`)
built from the *positive* factors only. `distance` included only when an origin is present; the
rest are location-intrinsic. Missing/neutral data → factor omitted.

Rate-limit defaults (constants in `community.rs`, keyed `contribution:{kind}:user:{id}` and, where
noted, `:ip:{ip}`):

```
parking-create  5/day/user  + 10/day/IP
edit           15/hour/user
proposal        5/hour/user
review         10/hour/user
verification   30/hour/user
parked-here    20/hour/user
```

`ContributionError` variants: `NotVerified`, `RateLimited`, `VersionConflict`,
`InvalidField(…)/Domain`, `NotFound`, `Unauthorized`, `Internal`. All mapped by the web layer to
friendly, non-leaking messages.

---

## 6. Infrastructure (crates/infrastructure)

- `timezone/offline.rs` — `OfflineTimezoneResolver` (timezonefinder-style polygon dataset; Rust
  port, e.g. `tzf-rs` — **confirm crate + data size at implementation**). Ledger #16.
- `community/contribution.rs` — `SqlxParkingContributionRepository`:
  `create` (insert location + revision v1 in one tx), `apply_edit` (optimistic `UPDATE … WHERE
  version = $expected` + revision insert + `last_meaningful_update_at` in one tx), `create_proposal`,
  `revision_history`, `duplicate_candidates` (`ST_DWithin` + name/address fetched for Rust-side
  similarity ranking).
- `community/review.rs` — `SqlxReviewRepository`: `upsert_review` (insert-or-update + append
  `review_revision` + recompute `rating_avg`/`rating_count` from ACTIVE reviews, one tx).
- `community/verification.rs` — `SqlxVerificationRepository`: `record`; `latest_existence_per_user`
  (`DISTINCT ON (user_id) … ORDER BY user_id, created_at DESC`); `attribute_summary`;
  `parked_here_count`; `mark_verified_at`.
- `community/favorite.rs` — `SqlxFavoriteRepository`.
- `community/history.rs` — `SqlxContributionHistoryReader` for C5.

`test-support` additions: `ReviewBuilder`, `VerificationBuilder`, `FavoriteBuilder`,
`RevisionBuilder` (or extend `ParkingBuilder` with `creator_id`/`version`). The existing
transaction/SAVEPOINT/committed-fixture harness is reused; read-model tests that query on other
pool connections use the **committed-fixture pattern** established in M1.

---

## 7. Web layer (crates/web)

### Middleware / gates

- `crates/web/src/auth.rs`: add `Auth::require_verified()` — `require_user()` + `is_verified`,
  else 403 (HTMX fragment) / redirect with the "verify your email to contribute" banner.
- CSRF: all new POST routes carry the session token (hidden input / `X-CSRF-Token`), same as M2.
- Rate limiting applied inside the handlers via the `ContributionService` (per §5).

### Routes

| Route | Method | Page/action | Access |
|---|---|---|---|
| `/parking/new` | GET/POST | D1 add location | authenticated + verified |
| `/parking/{id}/edit` | GET/POST | D2 edit (reversible fields) | authenticated + verified |
| `/parking/{id}/proposal` | POST | D2 sensitive change (move / removal) | authenticated + verified |
| `/parking/{id}/review` | GET/POST | D3 write/edit review | authenticated + verified |
| `/parking/{id}/verify` | POST | D4 verification signals (HTMX) | authenticated + verified |
| `/parking/{id}/parked-here` | POST | D4 "I parked here" (HTMX) | authenticated + verified |
| `/parking/{id}/favorite` | POST | favorite toggle (HTMX) | authenticated |
| `/account/favorites` | GET | C4 favorites list | authenticated |
| `/account/contributions` | GET | C5 contribution history | authenticated |

`/parking/{id}` (P3) gains: reviews list + summary, "recommended because…", confidence/verification
panel, favorite button, and the D3/D4 contributor actions (gated by `require_verified` /
authenticated).

### Templates / i18n

- New pages: `pages/{parking_new, parking_edit, review_form, favorites, contributions}.html`; new
  partials: `review_card`, `verification_panel`, `recommendation_reasons`, `duplicate_warning`,
  `favorite_button`, `confidence_badge`.
- P3 additions: review list + inline review form (HTMX swap), verification buttons
  ("still exists / no longer exists / information changed", per-attribute verify, "I parked here"),
  favorite toggle (`hx-post` + swap of the button state), "recommended because…" block.
- D1: map pin (Alpine) + address→coordinate geocode (HTMX), timezone field pre-filled and editable,
  duplicate-warning banner on submit (advisory, non-blocking).
- D2: editable fields pre-filled, hidden `version` input, separate "move the pin" / "propose removal"
  actions.
- HTMX conventions from M1/M2: fragments detect `HX-Request`, error fragments are swap-safe
  (§116.6), no inline `<script>` (Ledger #15).
- **i18n additions** (`crates/web/src/i18n.rs`): full en/pt-BR for D1–D4, C4–C5, the P3 additions,
  confidence labels, recommendation reason labels, duplicate-warning text, and all validation/error
  messages. Strings stay in the web catalog, never in domain/application logic (§12/§102).
- The design screens `d1…d3`, `c4`, `c5` are the visual contract; Tailwind utilities against the M0
  `@theme` tokens, matching M1/M2.

---

## 8. Seeder / commands

No new commands. `seed-mock` keeps working (its rows get `version = 1` from the column default and
no `creator_id`). Optionally, `seed-mock` may gain a couple of sample reviews/verifications to make
the P3 panel visible in dev — **if added, this is mock data and a Ledger entry** (tracked under
#1). Default: do not add sample reviews; keep M3 demo data created through the real forms.

---

## 9. Testing

| Layer | Tests |
|---|---|
| domain | `StarRating`/`ReviewBody` boundaries; verification/proposal/change-kind code round-trips; `confidence` every branch (empty, positives by freshness, `no_longer_exists` → Conflicting, `info_changed` → disputed-not-Conflicting) |
| application | `AddParkingLocation` (verified gate, dedup advisory, timezone auto+override, rate-limit); `ApplyParkingEdit` (optimistic conflict → `VersionConflict`, revision written, sensitive field rejected); `ProposeLocationChange`; `UpsertReview` (one-per-user, recompute, edit appends history); `RecordVerification` (`still_exists` sets `last_verified_at`, others don't, parked-here expiry); `ToggleFavorite`; confidence with fake repo |
| infrastructure (`#[db_test]`) | revision `UNIQUE(location_id, version)` + history reconstruction; optimistic `apply_edit` (concurrent: two edits on same version → exactly one wins); proposal create + status transitions; review unique constraint + `review_revision` chain; rating recompute equals direct `COUNT/AVG`; verification `DISTINCT ON` latest-per-user; favorite PK round-trip |
| web (`#[db_test]`) | unauthenticated → redirect on all new routes; unverified → blocked with banner; verified → add location (→ P3), edit reversible field (revision in C5), pin-move → PENDING proposal with no live change, review create (P3 aggregate updates), favorite toggle (C4 lists), `still_exists` (confidence Verified/Recently), conflicting signals → P3 shows Conflicting; CSRF on all POSTs; rate-limit → 429 |
| security (§60) | authorization-boundary tests: no new route reachable without the required gate; verification/favorites never render another user's identity; `creator_id`/author emails never appear in any rendered HTML |

---

## 10. Task breakdown

1. `0007_contributions.sql` + `0008_community.sql`; verify `cargo run` applies them; update
   `seed-mock` if the added columns need any touch-up.
2. Domain: `community.rs` (value objects, `Confidence` + rule, `ParkingLocation.version`) + unit
   tests.
3. Application: `timezone.rs` port, `community.rs` ports + `ContributionService` + rate-limit
   defaults + `RecommendationExplanation` (§105, alongside the M1 scorer) + tests with fakes.
   (`cargo add` the timezone-lookup crate; `cargo add` only, §11.)
4. Infrastructure: `OfflineTimezoneResolver`; the four `Sqlx*` repositories + history reader;
   `test-support` builders; `#[db_test]` integration tests.
5. Web: `require_verified` gate; handlers + route wiring; templates/partials for D1–D4, C4, C5 + P3
   additions; HTMX interactions; i18n additions; Tailwind classes matching the design screens.
6. HTTP/security tests; README (new routes, verified-gate behavior, timezone resolver note,
   rate-limit env if any); Ledger entries; live acceptance walkthrough against `docker compose` +
   a registered+verified user.

## 11. Risks / notes

- **Timezone resolver crate/data** — confirm the crate choice and bundled data size at
  implementation; fall back to a coarse country-capital table for points outside the dataset
  coverage (a Ledger entry if shipped).
- **Optimistic concurrency UX** — a 409 on edit must re-render D2 with the latest values + a clear
  "someone else changed this" notice; never a bare error page.
- **Rating aggregate drift** — recomputed in-transaction; the parity test (recompute == direct
  `COUNT/AVG`) is the guard against drift as reviews/hides accrue.
- **Confidence is read-time** — fine at single-location scale; revisit a denormalized column in M7
  perf validation.
- **Identity leaks** — verification/review/`creator_id` must never surface emails or OAuth subjects;
  render counts/labels only (§39/§41/§46). Add an explicit HTML-absence assertion.
- **`parked_here` expiry is latent in M3** — `expires_at` is set but the purge job is M6; the data
  is never read into any public list in M3.
- **htmx 4 4xx-swap** — new POST endpoints must return swap-safe fragments on error (§116.6);
  reuse M2's error-partial approach.
- **CSP** — no new inline `<script>`; keep Alpine expressions out of server-rendered
  verification/review data (Ledger #15).

---

## Ledger additions this milestone

| # | Item | Kind | Introduced | Remove/improve by | Notes |
|---|---|---|---|---|---|
| 16 | `OfflineTimezoneResolver` (bundled polygon data) | improve/dev | M3 | M7 | Real offline resolver; re-evaluate against a provider reverse-timezone; keep as fallback |
| 17 | Confidence-state thresholds + conflict rule hardcoded in domain | improve | M3 | M7 | Make configurable like `FreshnessConfig`; document for tuning |

Also updated: **Ledger #6** now covers contribution limits (parking/edit/proposal/review/
verification/parked-here) in addition to the M2 auth limits. **Ledger #9** (review-side freshness
thresholds) is now exercised by the confidence rule's `Verified`/`RecentlyVerified`/`Stale` split.
