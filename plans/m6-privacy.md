# M6 — Privacy & account lifecycle — implementation plan

> **Status: implemented.** Derived from `PLAN.md` (M6) and
> `REQUIREMENTS.md` (§20, §47, §66–§82, §98). Parent plan: `PLAN.md`.
>
> Implements the full milestone: migrations `0012`/`0013`; domain `privacy.rs`;
> application `privacy.rs` (ports + `PrivacyService` + `RetentionJob`);
> infrastructure `privacy/*` repos + `retention`/`seed-policies` CLI commands;
> the web routes/templates for P4/P5/P6 (+versions), C6, C7 (+single-use
> download), `/account/delete`, and `/admin/privacy-requests`; policy
> placeholders + docs; and a green test suite (domain/application/infra/web).

Companion to `REQUIREMENTS.md` (§66–§82 drive this milestone), `PLAN.md` (M6 overview) and
`UI_DESIGN.md` + `design-project/` screens `p4-privacy.html`, `p5-terms.html`, `p6-cookies.html`,
`c6-privacy-data.html`, `c7-export-status.html` — the visual contract for the privacy/legal pages
and the account privacy hub.

**What already exists:** `AccountState::Deleted` + the `can_log_in`/`can_access_account` gates that
block deleted/suspended accounts (M2, enforced mid-session in M5); `AuthService::suspend_user` /
`restore_user` with `revoke_all_for_user_except` (M5); the `audit_events` table + `AuditLog` port
(`record`) and `AuditLogReader`/admin viewer (M2/M5) — privacy actions are just new `action` codes
on the existing trail; `ObjectStorage::delete` (M1, used by M4 rejects) — the retention media sweep
reuses it; `parking_photo.uploader_id`, `review_photo.uploader_id`, `parking_location.creator_id`,
`parking_revision.editor_id` already nullable with `ON DELETE SET NULL` — photo/creator attribution
anonymizes cleanly; `sessions.expires_at`/`revoked_at`, token `expires_at`, `verification.expires_at`
(parked_here) already exist — retention only needs `DELETE` statements, not new columns. M6 is the
**privacy workflow + anonymization + retention + versioned legal pages + the two inventories** work
on top of that foundation.

**Goal:** data-subject rights and compliant account termination. A user can export their data, request
deletion, and have their account anonymized while community contributions remain (unattributed);
privacy pages are versioned and complete; retention jobs are testable; the data-processing and
provider data-flow inventories exist and mark legal bases for review.

**Working app means (acceptance):** a user requests an export and downloads it from an expiring,
single-use, owner-only link; a user deletes their account → they are logged out, `users` is scrubbed
(`account_state = DELETED`, email replaced with a non-attributable value), sessions/identities/
favorites/parked-here are gone, while their reviews/verifications/proposals/created locations remain
visible but unattributed (creator/author FKs `NULL`); the deletion request is recorded and audited;
the last admin cannot delete their own account; P4/P5/P6 render versioned text with an effective
date; `/admin/privacy-requests` lists requests for the manual rights (rectification/restriction/
objection/consent-withdrawal); `cargo run -p bikenest-web -- retention` purges expired sessions/
tokens/parked-here/exports and reports counts; the data-processing inventory and provider-transfer
inventory are complete with legal bases marked for review. `cargo test` green; fresh-clone onboarding
from README still works.

---

## 1. Scope

### In scope

| Area | Content |
|---|---|
| Schema | `0012_privacy.sql`: `privacy_request`, `personal_data_export`, `consent_record`; `users.deleted_at`; relax the four NOT-NULL/`ON DELETE CASCADE` attribution FKs. `0013_policies.sql`: `policy_version`. |
| Domain | `PrivacyRequestKind`/`PrivacyRequestState`/`ExportState`/`PolicyKind` code lists; `anonymized_email` helper; `RetentionPolicy` TTL constants; export-payload `schema_version` marker |
| Application | `privacy.rs`: `ExportService` (request + single-use download), `DeletionService` (anonymize-in-place), `PrivacyRequestService` (manual rights), `RetentionJob`; ports `ExportRepository`, `PrivacyRequestRepository`, `AnonymizationRepository`, `RetentionRepository`; new audit actions |
| Infrastructure | `SqlxExportRepository` (assemble payload across tables + row CRUD), `SqlxPrivacyRequestRepository`, `SqlxAnonymizationRepository` (one transaction), `SqlxRetentionRepository` (purge statements + media sweep), policy reader |
| Web | P4/P5/P6 (+ version history); C6 privacy hub; C7 export status/download; `/account/delete` confirmation; admin privacy-request queue; i18n additions |
| Gating | `require_user` for C6/C7/export/deletion/requests; deletion re-auth + last-admin guard; `require_role(Admin)` for the request queue; public legal pages |
| Documentation | Data-processing inventory (§68) + legal-basis mapping (§69); provider & international-transfer inventory (§76/§77); retention policy (§75); cookie inventory (§78) — all in this plan (§9–§12), exported to `docs/` |

### Explicitly out of scope (deferred, with where it lands)

| Item | Lands in |
|---|---|
| Real S3 / real `MEDIA_SIGNING_SECRET`; shared/Redis rate limiter; making the new TTLs configurable | M7 (Ledger #6/#7/#14 + new #20) |
| Scheduled execution of the retention job (cron / K8s CronJob / sidecar) | M7 deployment |
| SEO `hreflang` + strict CSP over the new privacy pages | M7 (§64/§65, §102, Ledger #15) |
| Face/license-plate auto-detection in photos (§80 "MAY be introduced later") | not required for initial release |
| A consent banner / cookie-preference manager for optional cookies | not required (no non-essential cookies shipped — §78) |
| Incident-response *automation* (§81) | M7 documents the runbook; M6 only guarantees the audit/log basis |
| Backup/restore interaction with deletion (§98) | M7 deployment (backups/restore are documented there) |
| Async export generation (`PROCESSING` worker) | only if export volume grows — synchronous now (§2) |

---

## 2. Decisions

| Decision | Choice | Reasoning |
|---|---|---|
| **Deletion model** | **Anonymize-in-place** ("retain-anonymized-contributions", PLAN M6 / §74). The `users` row survives with PII scrubbed; `account_state = DELETED`; `deleted_at` set. We never hard-delete a user in the product flow | §74 default model. Community content (reviews, verifications, proposals, created locations, photos) is *retained unattributed*, so we must keep the row's id stable for FKs — NULLing attribution columns rather than deleting rows is the only way to keep those rows without a user |
| **Anonymized email value** | `deleted+{user_id}@bikenest.invalid` — deterministic, unique per id (preserves the `lower(email)` unique index), and non-attributable. `.invalid` is RFC 2606-reserved so it can never receive mail | Must satisfy both the unique index and §74 "remove personal identity". Deterministic (no randomness → resume-safe, idempotent) |
| **Attribution anonymization** | Relax `review.author_id`, `verification.user_id`, `parking_proposal.proposer_id`, `report.reporter_id` from `NOT NULL`+`ON DELETE CASCADE` to nullable + `ON DELETE SET NULL`, then `UPDATE … SET <col> = NULL` during anonymization. Already-nullable columns (`parking_location.creator_id`, `parking_revision.editor_id`, `parking_photo.uploader_id`, `review_photo.uploader_id`, `report.claimed_by/resolved_by`, `audit_events.actor_user_id`) are simply set NULL | Community content must survive (reviews/verifications/proposals/reports/creations are the dataset); private *activity* and *identity* must not. `SET NULL` on the FK is the backstop for any future hard-delete path |
| **Parked-here vs other verification** | `verification.kind = 'parked_here'` rows are **deleted** (private activity); `existence`/`attribute` rows are **retained** with `user_id = NULL` | §74 "remove private activity" vs "retain non-personal community content". Parked-here is a personal "I was here" signal; existence/attribute is community curation |
| **Favorites** | **Deleted** | §42 favorites are private; §74 remove private activity |
| **Export generation** | **Synchronous** — the request handler assembles the JSON and stores it, state `READY` immediately. No worker/queue | Data volumes are small (a handful of rows per user). An async `PROCESSING` stage is premature; the state machine keeps `ExportState` extensible if it ever becomes necessary |
| **Export payload** | A versioned JSON document (`schema_version: 1`) assembled from all personal data (§73 machine-readable). Includes account, linked providers (provider + `subject`, **never** `credential_hash`), sessions (timestamps only), favorites, reviews (+revisions), verifications, proposals, reports, uploaded photos (metadata + storage keys — **not binary**). Excludes credential hashes, session/token hashes, CSRF tokens, and audit-log rows | §67 data elements; §73 "only data the user is authorized to receive". Secrets are excluded by construction; audit events are operational records (§47), disclosed via the access workflow not the export |
| **Download link** | `/account/export/{id}/download?token=…` — requires (a) the authenticated **owner** session and (b) an unexpired, **single-use** token (32 random bytes, SHA-256 stored). First successful download marks `DOWNLOADED` + clears the token; retention job expires after 24h | §73 "expire; require authentication or equivalent; not publicly indexable; not permanently accessible". Auth + token is two independent gates; single-use prevents replay |
| **Deletion re-authentication** | Password accounts must re-enter their current password; OAuth-only accounts (no password) re-confirm by typing their email + the active session is the authentication factor. Both confirm by typing the account email | §72 "verify identity/authority of the requester" before destructive disclosure. Password is the standard factor; OAuth-only accounts have no app password, so the session (already 2FA'd upstream at Google) + email confirmation is the pragmatic factor |
| **Last-admin guard** | Deleting (or anonymizing) the last `ADMIN` is rejected with the same `LastAdmin` error used by role revocation (M2) | Prevents a lock-out where no admin remains to operate moderation/audit (§19). Mirrors the existing `revoke_role` guard |
| **Manual vs automatic rights** | **Automatic:** access (export), deletion, export/portability. **Manual (recorded + operator-fulfilled):** rectification of non-self-serve fields, restriction, objection, consent-withdrawal | §72 "define which requests can be fulfilled automatically versus manually". The app has almost no consent-based processing and no automated decision-making, so the manual set is small and mostly a compliance-record surface |
| **`consent_record`** | A table that exists to record + support withdrawal, **initially empty** — the initial release uses no consent-based processing (§78 ships no non-essential cookies) | Honest §69 posture: we don't invent a consent flow that has nothing to consent to. The table + C6 "consent records" section is the ready surface for when consent-based processing is added |
| **Policy pages** | `policy_version` table keyed `(kind, version)` with `effective_at`/`superseded_at`; content seeded from `policies/*.md` via a `seed-policies` command, **marked as placeholder legal text requiring legal review** (§71) | §70 versioned so we can determine what was presented; §71 "legal text is product/legal content, not assumed final". Storing content in the DB (not templates) makes versioning + effective-dates real |
| **Cookie inventory** | Documented, not enforced by code: `session_id` (HttpOnly, Secure, SameSite=Lax, 30d — necessary), `csrf` (HttpOnly, SameSite=Lax, 1h — necessary/security), `lang` (SameSite=Lax, 1y — functional preference). No third-party, no analytics/tracking | §78. Inventory is a disclosure artifact + the P6 page content; no consent machinery needed since none are optional/tracking |
| **Retention execution** | A CLI command `cargo run -p bikenest-web -- retention` running a `RetentionJob` use case; each step returns a purge count, the whole run is audited as `retention.purged` (actor = system). Scheduled execution is M7 | §75 + the "real infrastructure, testable journey" rule: a testable command + integration tests now; cron/scheduler wiring is a deployment concern |
| **Inactive-account & deleted-account hard-delete** | Config-gated, **default disabled** (`INACTIVE_ACCOUNT_ANONYMIZE_AFTER_DAYS=0`, `DELETED_ACCOUNT_PURGE_AFTER_DAYS=0`); the retention job honors them only when set. Marked for legal/product review | §75 requires policies for both but the periods are legal decisions, not engineering ones (§75 "identify all retention periods that require legal/product approval") |
| **Audit actions** | New codes on the existing trail: `privacy.export_requested`, `privacy.export_downloaded`, `privacy.request_created`, `privacy.request_fulfilled`, `account.deletion_requested`, `account.anonymized`, `retention.purged` | §47 + §72 "privacy requests MUST be auditable". No new mechanism, just codes |
| **Compile-time SQL** | Continue `query_as!`/`query!` for all new readers/writers | §9/§305, established M1–M5 |

---

## 3. Schema

### `migrations/0012_privacy.sql`

```sql
-- §72/§73/§74: privacy requests, exports, consent records, and the
-- anonymize-in-place columns. See plans/m6-privacy.md §2–§3.

-- The privacy-request workflow (§72). Retained after anonymization
-- (user_id → NULL) so the compliance record survives the account.
CREATE TABLE privacy_request (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id      BIGINT REFERENCES users(id) ON DELETE SET NULL,
    kind         TEXT NOT NULL CHECK (kind IN
                    ('access', 'rectification', 'deletion', 'export',
                     'restriction', 'objection', 'consent_withdrawal')),
    state        TEXT NOT NULL DEFAULT 'OPEN'
                 CHECK (state IN ('OPEN', 'IN_PROGRESS', 'COMPLETED', 'DECLINED')),
    details      JSONB NOT NULL DEFAULT '{}'::jsonb,  -- rectification fields / objection reasons
    fulfilled_by BIGINT REFERENCES users(id) ON DELETE SET NULL,
    fulfilled_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX privacy_request_user_idx  ON privacy_request (user_id, created_at DESC);
CREATE INDEX privacy_request_state_idx ON privacy_request (state, created_at);

-- Personal-data export (§73). Payload is stored (assembled synchronously) so
-- the download streams a snapshot, not a re-derivation. Single-use token.
CREATE TABLE personal_data_export (
    id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id        BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    state          TEXT NOT NULL DEFAULT 'READY'
                   CHECK (state IN ('READY', 'DOWNLOADED', 'EXPIRED')),
    token_hash     TEXT NOT NULL,              -- sha256_hex(raw download token)
    payload        JSONB NOT NULL,             -- ExportPayload, schema_version 1
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at     TIMESTAMPTZ NOT NULL,       -- now + 24h (§75)
    downloaded_at  TIMESTAMPTZ
);
CREATE INDEX personal_data_export_user_idx    ON personal_data_export (user_id, created_at DESC);
CREATE INDEX personal_data_export_expiry_idx  ON personal_data_export (expires_at)
    WHERE state = 'READY';

-- Consent records (§69). Empty in the initial release (no consent-based
-- processing — §78); the C6 "consent records" surface is ready for the future.
CREATE TABLE consent_record (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scope      TEXT NOT NULL,                 -- e.g. 'email_marketing', 'cookie_analytics'
    status     TEXT NOT NULL DEFAULT 'GRANTED'
               CHECK (status IN ('GRANTED', 'WITHDRAWN')),
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    withdrawn_at TIMESTAMPTZ
);
CREATE INDEX consent_record_user_idx ON consent_record (user_id, scope);

-- users gains the anonymization clock (§75 "deleted accounts" retention).
ALTER TABLE users ADD COLUMN deleted_at TIMESTAMPTZ;

-- Anonymize-in-place requires the attribution columns to be nullable so the
-- anonymization transaction can SET them NULL while keeping community rows.
-- Change the FK action to SET NULL as the backstop for any future hard-delete.
ALTER TABLE review DROP CONSTRAINT review_author_id_fkey;
ALTER TABLE review ALTER COLUMN author_id DROP NOT NULL;
ALTER TABLE review ADD CONSTRAINT review_author_id_fkey
    FOREIGN KEY (author_id) REFERENCES users(id) ON DELETE SET NULL;

ALTER TABLE verification DROP CONSTRAINT verification_user_id_fkey;
ALTER TABLE verification ALTER COLUMN user_id DROP NOT NULL;
ALTER TABLE verification ADD CONSTRAINT verification_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE SET NULL;

ALTER TABLE parking_proposal DROP CONSTRAINT parking_proposal_proposer_id_fkey;
ALTER TABLE parking_proposal ALTER COLUMN proposer_id DROP NOT NULL;
ALTER TABLE parking_proposal ADD CONSTRAINT parking_proposal_proposer_id_fkey
    FOREIGN KEY (proposer_id) REFERENCES users(id) ON DELETE SET NULL;

ALTER TABLE report DROP CONSTRAINT report_reporter_id_fkey;
ALTER TABLE report ALTER COLUMN reporter_id DROP NOT NULL;
ALTER TABLE report ADD CONSTRAINT report_reporter_id_fkey
    FOREIGN KEY (reporter_id) REFERENCES users(id) ON DELETE SET NULL;
```

### `migrations/0013_policies.sql`

```sql
-- §70/§71: versioned legal pages. Content is seeded from policies/*.md via
-- `seed-policies`; text is PLACEHOLDER legal content requiring review (§71).
CREATE TABLE policy_version (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind          TEXT NOT NULL CHECK (kind IN ('privacy', 'terms', 'cookies')),
    version       TEXT NOT NULL,              -- e.g. '2025-01-01.1'
    effective_at  TIMESTAMPTZ NOT NULL,
    superseded_at TIMESTAMPTZ,                -- NULL = current
    content       TEXT NOT NULL,              -- markdown, rendered escaped
    UNIQUE (kind, version)
);
CREATE INDEX policy_version_kind_idx ON policy_version (kind, effective_at DESC);
```

Notes:

- The four FK drops use Postgres's auto-generated names (`<table>_<column>_fkey`); the migration is
  forward-only, applied once (same pattern as M5's `parking_photo_moderation_state_check`).
- No backfill: all four columns are populated by definition (every row has an author/proposer/
  reporter), so dropping `NOT NULL` is lossless; the anonymization transaction is what NULLs them.
- `personal_data_export.user_id` is `ON DELETE CASCADE` as a backstop, but the product flow deletes
  exports explicitly during anonymization and never hard-deletes the user row.
- `seed-mock`/`seed-admin` are unaffected (they don't touch the new tables; the widened FKs accept
  their existing rows unchanged).

---

## 4. Domain model (crates/domain)

New module `crates/domain/src/privacy.rs` (pure, no I/O):

```
PrivacyRequestKind  { Access, Rectification, Deletion, Export,
                      Restriction, Objection, ConsentWithdrawal }   // as_code/from_code
PrivacyRequestState { Open, InProgress, Completed, Declined }       // as_code/from_code
ExportState         { Ready, Downloaded, Expired }                  // as_code/from_code
PolicyKind          { Privacy, Terms, Cookies }                     // as_code/from_code

/// Deterministic, non-attributable, unique replacement email for an anonymized
/// account (RFC 2606 `.invalid` so it can never receive mail).
pub fn anonymized_email(user_id: UserId) -> String  // "deleted+{id}@bikenest.invalid"

/// Retention TTLs (§75). Hardcoded now (Ledger #20 → configurable in M7);
/// the same constants used at issue-time (M2/M3) are the single source of truth.
pub struct RetentionPolicy {
    pub password_reset_ttl: Duration,     // 1 hour
    pub email_verification_ttl: Duration, // 24 hours
    pub session_idle: Duration,           // 30 days (cookie Max-Age parity)
    pub parked_here_ttl: Duration,        // 90 days
    pub export_ttl: Duration,             // 24 hours
    pub upload_orphan_ttl: Duration,      // 24 hours
}
impl Default for RetentionPolicy { … }
```

`auth.rs` addition: `AccountState::Deleted` gains no new behavior (its `can_log_in`/`can_access_account`
already return false), but `User` is documented as the shape *before* anonymization; the anonymized
shell is never reconstructed as a `User`.

Domain unit tests: enum round-trips for the four new code lists; `anonymized_email` is deterministic,
unique across ids, and contains no input-email substring; `RetentionPolicy::default` matches §75's
suggested technical defaults; `AccountState::Deleted` gates unchanged.

---

## 5. Application layer (crates/application)

New module `crates/application/src/privacy.rs`; `auth.rs` gains `revoke_all_for_user` (no keep);
`lib.rs` re-exports.

### Ports

```rust
// privacy.rs — export read/write (§73)
pub struct NewExport { user_id, token_hash, payload: ExportPayload, expires_at }
pub struct Export { id, user_id, state, created_at, expires_at, downloaded_at }   // no payload here
pub struct ExportDownload { payload: ExportPayload }                                // payload only on download

#[async_trait] trait ExportRepository: Send + Sync {
    async fn assemble_payload(&self, user_id: UserId) -> Result<ExportPayload, PrivacyError>;
    async fn create(&self, e: &NewExport) -> Result<i64, PrivacyError>;
    async fn list_for_user(&self, user_id: UserId) -> Result<Vec<Export>, PrivacyError>;
    async fn get(&self, id: i64) -> Result<Option<Export>, PrivacyError>;
    /// Validates token (constant-time hash compare) + not expired + not downloaded;
    /// marks DOWNLOADED on success. Returns the payload once.
    async fn consume_download(&self, id: i64, token_hash: &str, now: DateTime<Utc>)
        -> Result<Option<ExportDownload>, PrivacyError>;
    async fn purge_expired(&self, now: DateTime<Utc>) -> Result<u64, PrivacyError>;
}

// privacy.rs — manual rights workflow (§72)
pub struct NewPrivacyRequest { user_id, kind, details: serde_json::Value }
pub struct PrivacyRequest { id, user_id: Option<UserId>, kind, state, details, created_at, updated_at }

#[async_trait] trait PrivacyRequestRepository: Send + Sync {
    async fn create(&self, r: &NewPrivacyRequest) -> Result<i64, PrivacyError>;
    async fn list(&self, state: Option<PrivacyRequestState>) -> Result<Vec<PrivacyRequest>, PrivacyError>;
    async fn fulfill(&self, id: i64, by: UserId) -> Result<(), PrivacyError>;   // OPEN/IN_PROGRESS → COMPLETED
}

// privacy.rs — anonymize-in-place (§74), one transaction
pub struct AnonymizationReport { /* per-table counts: identities, sessions, tokens,
    favorites, parked_here, reviews, verifications, proposals, reports, photos, locations, revisions, audit */ }

#[async_trait] trait AnonymizationRepository: Send + Sync {
    async fn is_last_admin(&self, user_id: UserId) -> Result<bool, PrivacyError>;
    async fn anonymize(&self, user_id: UserId, now: DateTime<Utc>) -> Result<AnonymizationReport, PrivacyError>;
}

// privacy.rs — retention job (§75)
#[async_trait] trait RetentionRepository: Send + Sync {
    async fn purge_expired_password_reset_tokens(&self, now) -> Result<u64, PrivacyError>;
    async fn purge_expired_email_verification_tokens(&self, now) -> Result<u64, PrivacyError>;
    async fn purge_expired_sessions(&self, now) -> Result<u64, PrivacyError>;
    async fn purge_expired_parked_here(&self, now) -> Result<u64, PrivacyError>;
    async fn purge_expired_exports(&self, now) -> Result<u64, PrivacyError>;
    async fn purge_orphan_uploads(&self, now) -> Result<u64, PrivacyError>;      // media sweep (below)
    async fn anonymize_inactive_accounts(&self, cutoff) -> Result<u64, PrivacyError>;  // config-gated
    async fn purge_deleted_accounts(&self, cutoff) -> Result<u64, PrivacyError>;       // config-gated
}
```

`ExportPayload` is a `#[derive(Serialize)]` application struct — the `schema_version: 1` document:
account block, `authentication` (provider + subject, no `credential_hash`), `sessions` (timestamps),
`favorites`, `reviews` (with revisions), `verifications`, `proposals`, `reports`, `photos` (metadata
+ keys). `PrivacyError` variants: `NotAuthorized`, `NotFound`, `LastAdmin`, `ReauthRequired`,
`InvalidToken`, `Expired`, `AlreadyDownloaded`, `InvalidKind`, `Internal`.

`auth.rs`: `SessionStore::revoke_all_for_user(user_id)` — the delete path's "invalidate sessions"
(no keep session to preserve), next to the existing `revoke_all_for_user_except`.

### Use cases

| Use case | Flow (abridged) |
|---|---|
| `PrivacyService::request_export(user)` | `require_user` → `assemble_payload` → generate token (32B) + hash → `create` (state `READY`, `expires_at = now + 24h`) → audit `privacy.export_requested` |
| `PrivacyService::list_exports(user)` | owner-only reader → C7 |
| `PrivacyService::download_export(user, id, token)` | `require_user` → **owner check** → `consume_download` (single-use, expiring) → audit `privacy.export_downloaded` → stream JSON |
| `PrivacyService::request_deletion(user, reauth)` | `require_user` → **re-authenticate** (password or OAuth-session+email) → **last-admin guard** → `create privacy_request(kind=deletion)` → `anonymize` (one tx) → mark request `COMPLETED` → audit `account.deletion_requested` + `account.anonymized` → revoke all sessions |
| `PrivacyService::submit_request(user, kind, details)` | `require_user` → validate kind ∈ manual set → `create` (state `OPEN`) → audit `privacy.request_created` |
| `PrivacyService::list_requests(actor)` / `fulfill_request(actor, id)` | `require_role(Admin)` → `list`/`fulfill` (→ `COMPLETED`) → audit `privacy.request_fulfilled` |
| `RetentionJob::run()` | loop the eight `RetentionRepository` steps (each returns a count); assemble a summary; audit `retention.purged` (actor `None`, metadata = per-step counts). Config-gated steps skipped when their TTL = 0 |

`DeletionService` (or the `request_deletion` use case) drives the anonymization **transaction**
(§3 record mapping, §6 infra): scrub `users` (email → `anonymized_email`, `display_name = NULL`,
`email_verified_at = NULL`, `suspended_at = NULL`, `deleted_at = now`, `account_state = 'DELETED'`);
DELETE `authentication_identities`, `sessions`, `email_verification_tokens`, `password_reset_tokens`,
`user_roles`, `favorite`, `verification WHERE kind='parked_here'`, `personal_data_export`,
`consent_record`; SET NULL on `review.author_id`, `verification.user_id`, `parking_proposal.proposer_id`,
`parking_proposal.resolved_by`, `report.reporter_id`/`claimed_by`/`resolved_by`,
`parking_location.creator_id`, `parking_revision.editor_id`, `parking_photo.uploader_id`,
`review_photo.uploader_id`, `audit_events.actor_user_id`, `privacy_request.user_id`.

---

## 6. Infrastructure (crates/infrastructure)

- `privacy/export.rs` — `SqlxExportRepository`:
  `assemble_payload` runs one read per section (account + roles + identities; sessions; favorites;
  reviews + revisions; verifications; proposals; reports; photos via `parking_photo`/`review_photo`)
  and builds the `ExportPayload` (application-layer struct, serde); `create`/`list_for_user`/`get`
  are straight `query_as!`/`query!`; `consume_download` does a constant-time hash compare then
  `UPDATE … SET state='DOWNLOADED', downloaded_at=now() WHERE id=… AND state='READY' AND expires_at > now()`
  (`RETURNING payload`) — 0 rows → `Expired`/`AlreadyDownloaded`; `purge_expired` deletes
  `state='READY' AND expires_at < now()`.
- `privacy/request.rs` — `SqlxPrivacyRequestRepository`: `create`, `list` (optional state filter,
  ordered `created_at`), `fulfill` (`UPDATE … WHERE state IN ('OPEN','IN_PROGRESS')` → `COMPLETED`,
  0 rows → error).
- `privacy/anonymize.rs` — `SqlxAnonymizationRepository`: `is_last_admin` (count of
  `user_roles WHERE role='ADMIN' AND user_id <> …`); `anonymize` = one `sqlx` transaction performing
  the §5 operation list, returning an `AnonymizationReport` with per-table row counts (via
  `result.rows_affected()`). `revoke_all_for_user` lives in the session repo (`sessions SET
  revoked_at=now() WHERE user_id=…`).
- `privacy/retention.rs` — `SqlxRetentionRepository`: the eight purge statements
  (`DELETE FROM … WHERE expires_at/revoked_at < now()`; parked_here uses the partial index
  `verification_parked_expiry`; exports use `personal_data_export_expiry_idx`). `purge_orphan_uploads`
  lists all object keys referenced by `parking_photo.storage_key`/`thumbnail_key` +
  `review_photo.storage_key`/`thumbnail_key`, walks the media root, and `ObjectStorage::delete`s
  unreferenced files older than 24h (best-effort, returns count). `anonymize_inactive_accounts` /
  `purge_deleted_accounts` are no-ops returning 0 when their config TTL = 0.
- `privacy/policy.rs` — `SqlxPolicyReader`: `current(kind)` (latest `effective_at`, `superseded_at
  IS NULL`), `history(kind)` (all versions ordered `effective_at DESC`).
- `auth/account_repo.rs` + `auth/session_repo.rs` — add `revoke_all_for_user`.
- Config: extend `config.rs` with `ExportTtlHours`, `InactiveAccountAnonymizeAfterDays`,
  `DeletedAccountPurgeAfterDays` (+ `.env.example` entries), defaulted to §75 values / `0`.

`test-support` additions: `PrivacyRequestBuilder`, `ExportBuilder`, and an `ExistingUserBuilder`
reuse so anonymization tests can assert FK-nulling across all tables; reuse the transaction/
SAVEPOINT/committed-fixture harness (M1).

---

## 7. Web layer (crates/web)

### Middleware / gates

- `require_user` (M2) already blocks `Deleted` via `can_access_account`; no change needed for the
  post-deletion "cannot act" guarantee. Deletion itself runs while the account is still `Active`.
- `require_role(Role::Admin)` for `/admin/privacy-requests`.
- Re-authentication is enforced in the application service (server-side), not just the form.
- CSRF on every new POST; htmx-4 4xx-swap-safe fragments on error (§116.6). No inline `<script>`
  (Ledger #15); the delete-confirmation and export forms are plain forms, not Alpine.

### Routes

| Route | Method | Page/action | Access |
|---|---|---|---|
| `/privacy`, `/terms`, `/cookies` | GET | P4/P5/P6 (current version + effective date) | public |
| `/privacy/versions`, `/terms/versions`, `/cookies/versions` | GET | version history (§70 determinability) | public |
| `/account/privacy` | GET | C6 privacy hub | authenticated |
| `/account/privacy/export` | POST | request export → redirect to C7 | authenticated |
| `/account/export/{id}` | GET | C7 export status | owner only |
| `/account/export/{id}/download` | GET | download (token param, single-use) | owner + token |
| `/account/delete` | GET | deletion confirmation form | authenticated |
| `/account/delete` | POST | perform deletion (re-auth + confirm) | authenticated |
| `/account/privacy/request` | POST | manual rights request (rectification/restriction/objection/consent-withdrawal) | authenticated |
| `/admin/privacy-requests` | GET | privacy-request queue | ADMIN |
| `/admin/privacy-requests/{id}/fulfill` | POST | mark a manual request COMPLETED | ADMIN |

- `/account` (C1) gains a link to `/account/privacy`; after deletion the user is redirected to
  `/login?deleted=1` (session revoked, so `require_user` blocks re-entry to C-pages).
- Download responses set `Content-Type: application/json` + `Content-Disposition: attachment` and are
  `noindex` (non-indexable — §73).

### Templates / i18n

- New pages: `pages/privacy.html`, `pages/terms.html`, `pages/cookies.html`, `pages/privacy_versions.html`,
  `pages/account_privacy.html` (C6), `pages/account_export.html` (C7), `pages/account_delete.html`,
  `pages/admin_privacy_requests.html`. `pages/account.html` gains a privacy nav link.
- New partials: `privacy_request_result`, `export_row`, `privacy_request_row`, `delete_confirm`.
- Policy content is rendered **escaped** (Askama default — never `|safe`), with the markdown rendered
  as plain paragraphs/headings from the stored content (no raw-HTML injection from `policy_version`).
- **i18n additions** (`crates/web/src/i18n.rs`): full en/pt-BR for the export/delete/request flows
  (titles, buttons, statuses `READY`/`DOWNLOADED`/`EXPIRED`, expiry notices, single-use notice),
  the seven privacy-request kinds + four states, deletion confirmation/re-auth labels, the
  post-deletion notice, the policy-page nav + "effective date/version" labels, cookie-inventory rows,
  and all validation/error messages. Strings stay in the web catalog (§12/§102).
- The design screens `p4`, `p5`, `p6`, `c6`, `c7` are the visual contract; Tailwind utilities against
  the M0 `@theme` tokens, matching M1–M5.

---

## 8. Commands / config

- **`retention`** (`cargo run -p bikenest-web -- retention`): runs `RetentionJob`, prints the
  per-step counts, audits `retention.purged`. Scheduled externally in M7.
- **`seed-policies`** (`cargo run -p bikenest-web -- seed-policies`): idempotently upserts the
  current `policies/privacy.md`, `policies/terms.md`, `policies/cookies.md` as new `policy_version`
  rows (superseding any placeholder without an effective date conflict), reading `POLICY_VERSION`
  + `POLICY_EFFECTIVE_AT` from env (Ledger #21 — placeholder legal text awaiting review).
- `seed-mock`/`seed-admin` unchanged.
- New `.env.example` entries: `EXPORT_TTL_HOURS=24`, `INACTIVE_ACCOUNT_ANONYMIZE_AFTER_DAYS=0`,
  `DELETED_ACCOUNT_PURGE_AFTER_DAYS=0`, `POLICY_VERSION=2025-01-01.1`.

---

## 9. Data-processing inventory (§67/§68) + legal basis (§69)

The table below is the deliverable; it is exported to `docs/data-processing-inventory.md` as a living
document. **Every legal basis is marked for legal review** — the engineering plan must not invent them
(§69).

| Data element | Purpose | Legal basis (⚠ = legal review) | Req/Opt | Stored | Access | Retention | Recipients / transfer |
|---|---|---|---|---|---|---|---|
| email | auth identity, verification, reset, contact | ⚠ performance of contract / legitimate interest (account admin) | required | `users.email`, `authentication_identities.provider_subject` | self; admin (investigation) | until anonymization | email provider (delivery only) |
| password hash | password auth | ⚠ performance of contract | required | `authentication_identities.credential_hash` | never readable | deleted on anonymization | — (never transferred) |
| OAuth provider id (`google.sub`) | OAuth auth | ⚠ performance of contract | optional | `authentication_identities.provider_subject` | self | deleted on anonymization | Google (their own records) |
| display_name | optional profile | ⚠ consent / legitimate interest | optional | `users.display_name` | self (never public) | nulled on anonymization | — |
| session info (hash, timestamps) | session/CSRF | ⚠ legitimate interest (security) | required | `sessions` | never readable | 30d cookie / 90d cap; purged | — |
| IP address / user-agent | rate-limit keys, audit | ⚠ legitimate interest (security) | transient | in-memory limiter; not persisted beyond request | internal | not retained (§45) | — |
| reviews | community content | ⚠ legitimate interest (dataset) | optional | `review`/`review_revision` | public (attribution anonymized) | retained, anonymized | — |
| contributions (locations, proposals, revisions) | dataset | ⚠ legitimate interest | optional | `parking_location`/`parking_proposal`/`parking_revision` | public (unattributed) | retained, anonymized | — |
| verification activity | confidence signals | ⚠ legitimate interest | optional | `verification` | aggregated | existence/attribute retained anonymized; parked-here 90d | — |
| parked-here events | personal "I was here" | ⚠ legitimate interest | optional | `verification(kind=parked_here)` | never public | 90 days; deleted on account deletion | — |
| favorites | private bookmarks | ⚠ legitimate interest | optional | `favorite` | self only | deleted on account deletion | — |
| reports | moderation input | ⚠ legal obligation / legitimate interest | optional | `report` | moderators only | retained, reporter anonymized | — |
| photos + metadata | community content | ⚠ legitimate interest | optional | `parking_photo`/`review_photo` + object storage | public (uploader never shown) | retained, uploader anonymized | object-storage provider |
| browser geolocation | search origin (§79) | ⚠ consent (browser prompt) | optional | never persisted | client only | not retained | geocoder/map (coordinates only) |
| audit information | security/compliance | ⚠ legal obligation | required | `audit_events` | admin only | long-term (legal review) | — |
| privacy requests | rights workflow | ⚠ legal obligation | optional | `privacy_request` | admin only | legal-review period, user_id nulled | — |
| consent records | consent evidence | ⚠ legal obligation (where consent) | optional | `consent_record` | self/admin | retained while valid | — |

---

## 10. Provider & international-transfer inventory (§76/§77)

Exported to `docs/provider-transfer-inventory.md`. Documents, per provider, what crosses the boundary
and the §77 minimization guarantee.

| Provider | Purpose | Data transferred | Region | Role | Transfer mechanism | Retention | Deletion |
|---|---|---|---|---|---|---|---|
| Google (OAuth) | login | `sub`, email, `email_verified` (from Google) | ⚠ review | processor→controller | OAuth 2.0 | Google's own records | revoke/disconnect |
| Email provider (SMTP/Resend) | verification/reset mail | email, token link | ⚠ review | processor | TLS | provider-specific | provider DPA |
| Geocoding provider | address→coords | query string + coordinates (§77: no account identity) | ⚠ review | processor | HTTPS | request-scoped | — |
| Map/tile provider | render map | tile requests (§77: no authenticated identity) | ⚠ review | processor | HTTPS | request-scoped | — |
| Object-storage provider | photo binaries | derivative bytes under opaque keys (§77: no user metadata in keys) | ⚠ review | processor | HTTPS | until rejected/anonymized | `ObjectStorage::delete` |
| Hosting provider | run app | full app + DB | ⚠ review | processor | — | per SLA | standard |
| Observability / error tracking | logs/metrics | logs (no secrets; PII minimized) | ⚠ review | processor | — | per config | standard |

**§77 minimization confirmations (already true from M1–M5):** the map renderer receives no
authenticated identity (tiles are public); the geocoder receives only the query string; the object
store receives only derivative bytes under opaque keys (no email/subject in keys); the email provider
receives only the address + link. This milestone documents and test-asserts these boundaries (no code
change expected).

---

## 11. Retention policy (§75)

Exported to `docs/retention-policy.md`. **Bold = legal/product approval required.**

| Record | Period | Mechanism | Approval |
|---|---|---|---|
| password-reset token | 1 hour | expires_at + retention purge | technical default |
| email-verification token | 24 hours | expires_at + retention purge | technical default |
| session | 30 days idle / 90-day absolute cap | cookie Max-Age + `expires_at` + purge | technical default |
| "I parked here" | 90 days | `expires_at` + purge (and on deletion) | technical default |
| temporary privacy exports | 24 hours | `expires_at` + purge | technical default |
| temporary upload objects | 24 hours | orphan media sweep | technical default |
| **inactive accounts** | **config-gated, default off** | `INACTIVE_ACCOUNT_ANONYMIZE_AFTER_DAYS` | **⚠ legal/product** |
| **deleted (anonymized) account shells** | **config-gated, default off** | `DELETED_ACCOUNT_PURGE_AFTER_DAYS` | **⚠ legal/product** |
| **reviews / contributions / reports** | **long-term (retained anonymized)** | retained | **⚠ legal/product** |
| **audit events / security logs** | **long-term** | retained | **⚠ legal/product** |
| privacy requests | legal-review period | retained, `user_id` nulled | **⚠ legal/product** |

---

## 12. Testing

| Layer | Tests |
|---|---|
| domain | four code-list round-trips; `anonymized_email` determinism + uniqueness + non-attributability; `RetentionPolicy::default` equals §75 defaults |
| application | `request_export` (auth gate, payload assembled, `READY` + audit); `download_export` (owner-only, wrong token → `InvalidToken`, expired → `Expired`, second download → `AlreadyDownloaded`, audit); `request_deletion` (**last-admin guard**, re-auth failure → `ReauthRequired`, happy path → `account.anonymized` audit + all-session revoke); `submit_request`/`fulfill_request` (admin gate, wrong kind); `RetentionJob` (each step returns counts, config-gated steps skipped, `retention.purged` audit); with fakes |
| infrastructure (`#[db_test]`) | `assemble_payload` returns every section and **excludes** `credential_hash`/token hashes; export `consume_download` single-use + expiry (0-rows paths); **anonymize transaction**: scrub `users`, delete identities/sessions/tokens/favorites/parked-here/exports, NULL every attribution column, keep reviews/verifications(existence)/proposals/locations, and report correct per-table counts; retention purge statements delete only expired rows; policy reader returns current + history |
| web (`#[db_test]`) | public `/privacy`/`/terms`/`/cookies` render version + effective date; version history renders; C6/C7/export/deletion 401 for anonymous; `/account/export/{id}` + download 403 for non-owner; download `noindex` + JSON content type; delete POST → redirected to `/login?deleted=1`, session invalidated, re-login blocked, community content still visible with author `NULL`; last-admin delete blocked; `/admin/privacy-requests` 403 for non-admin; CSRF on all new POSTs; policy content HTML-escaped (XSS assertion) |
| security (§60/§61) | no password/token/PII key ever reaches the export payload or audit `metadata`; anonymized email never rendered publicly; creator/author/reporter identities absent from public pages after deletion; download link not indexable + single-use; deletion is auditable end-to-end; boundaries §77 asserted (no identity in tile/geocode/object-key calls) |

---

## 13. Task breakdown

1. `0012_privacy.sql` + `0013_policies.sql`; verify `cargo run` applies them; confirm `seed-mock`/
   `seed-admin` still run under the relaxed FKs.
2. Domain: `privacy.rs` (code lists + `anonymized_email` + `RetentionPolicy`) + unit tests.
3. Application: `privacy.rs` (ports + `ExportPayload` + `PrivacyService` + `DeletionService` +
   `RetentionJob` + `PrivacyError`); `auth.rs` `revoke_all_for_user`; `lib.rs` re-exports; tests
   with fakes.
4. Infrastructure: `SqlxExportRepository`, `SqlxPrivacyRequestRepository`,
   `SqlxAnonymizationRepository`, `SqlxRetentionRepository`, policy reader, `revoke_all_for_user`,
   config additions; `test-support` builders; `#[db_test]` integration tests.
5. Web: routes + gates; P4/P5/P6 + version pages; C6/C7; `/account/delete` confirmation; admin
   privacy-request queue; templates/partials; i18n; Tailwind matching the design screens.
6. `seed-policies` + `policies/*.md` placeholders (marked for legal review); `retention` command.
7. Docs: `docs/data-processing-inventory.md`, `docs/provider-transfer-inventory.md`,
   `docs/retention-policy.md` (from §9–§11); README (new routes, retention command, deletion flow);
   Ledger entries.
8. HTTP/security tests; live acceptance walkthrough against `docker compose`: request export →
   download; delete a contributing account → confirm anonymization + unattributed content + last-admin
   guard; run `retention`; browse versioned P4/P5/P6; admin sees the privacy-request queue.

## 14. Risks / notes

- **Anonymization transaction breadth** — it touches ~15 tables. Keep it a single `sqlx`
  transaction with a per-table `AnonymizationReport` so tests can assert completeness; a partial
  apply (identity scrubbed but a review still attributed) is the main correctness hazard.
- **FK constraint drops** — `review_author_id_fkey` etc. are auto-generated names; forward-only,
  applied once (same pattern as M5's CHECK drop/re-add). No backfill needed.
- **Uniqueness of the anonymized email** — `deleted+{id}@bikenest.invalid` is unique per id; do not
  use a constant string (the `lower(email)` unique index would reject the second anonymization).
- **Single-use download token** — hash compare must be constant-time; "downloaded" and "expired"
  must be distinguishable (a second attempt is `AlreadyDownloaded`, not a confusing `NotFound`).
- **Last-admin guard** must be server-side in `request_deletion`, mirroring the M2 `revoke_role`
  guard; test the exact scenario (sole admin deleting themselves).
- **Re-authentication for OAuth-only accounts** — the session *is* the factor; document this in the
  plan (done, §2) and don't silently require a password that doesn't exist.
- **Export payload secrets** — add a serialization guard/test that `credential_hash`, session/token
  hashes, and CSRF tokens never appear in the payload or audit `metadata`.
- **Policy content escaping** — `policy_version.content` is untrusted (legal-team-authored), render
  escaped; never `|safe`; keep `seed-policies` from overwriting a real reviewed version silently
  (only supersede when the version string changes).
- **Retention job idempotency** — every purge is a `DELETE WHERE expires_at < now()`; re-runs are
  no-ops returning 0, safe to schedule in M7 without extra guards.
- **Backup interaction (§98)** — documented in M7; M6 only guarantees the DB-level anonymization is
  a committed transaction so a restored backup doesn't resurrect a non-anonymized row mid-flight.

---

## Ledger additions this milestone

| # | Item | Kind | Introduced | Remove/improve by | Notes |
|---|---|---|---|---|---|
| 20 | Retention/export TTLs hardcoded (reset 1h, verification 24h, session 30d/90d, parked-here 90d, export 24h, upload-orphan 24h) | improve | M6 | M7 | Make configurable via `RetentionPolicy`/env, like Ledger #18/#19 |
| 21 | `seed-policies` placeholder legal text (privacy/terms/cookies) | placeholder | M6 | legal review (product) | §71: content is product/legal content, not assumed final text |

No existing Ledger entries change (privacy actions are not rate-limited — they are authenticated,
single-user, and audited, so Ledger #6 gains no new limits). No new fakes/mocks introduced.
