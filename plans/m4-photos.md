# M4 — Photos — implementation plan

> **Status: implemented (M4, alongside M2/M3).** Derived from `PLAN.md` (M4) and `REQUIREMENTS.md`
> (§30, §38, §44–§45, §80, §84, §116.2/§116.9). Parent plan: `PLAN.md`.

Companion to `REQUIREMENTS.md` (§30 drives this milestone), `PLAN.md` (M4 overview) and
`UI_DESIGN.md` + `design-project/` screen `m2-photos.html`, plus the P3 gallery and the D1
photo-attach field — the visual contract.

**What already exists (pulled forward to M1):** the `ObjectStorage` port + `LocalDiskStorage`
(signed, expiring `/media` URLs — Ledger #7), the `parking_photo` table, and the P3 gallery reader
(`SqlxParkingPhotoReader`, approved-only). M4 is the **upload → validate → process → moderate →
publish** work on top of that foundation, not storage itself.

**Goal:** the photo pipeline from authenticated upload to moderated publication. A verified user
uploads a photo; it is validated by content, re-encoded with EXIF stripped, thumbnailed, stored only
as processed derivatives, and held in `PENDING_REVIEW` until a moderator approves it; only then is
it publicly visible.

**Working app means (acceptance):** a verified user uploads a photo → it enters the moderator queue
as `PENDING_REVIEW` and is **not** in the P3 gallery → a moderator approves → the processed
derivative appears on the details page; rejection works (bytes deleted, row retained for audit);
a test asserts EXIF metadata is gone and the original upload is not reachable. `cargo test` green;
fresh-clone onboarding from README still works.

---

## 1. Scope

### Implementation notes

- **D1 photo-attach is deferred** (§10 task 5). The P3 add-photo endpoint
  (`POST /parking/{id}/photo`) is the primary path; a location is created first
  and its photo attached via P3 using the same `PhotoService::upload` pipeline.
  Converting `parking_new_post` to `multipart` is additive and can land as a
  follow-up; the risk it posed to the existing create form is avoided.

### In scope

| Area | Content |
|---|---|
| Schema | `0009_photos.sql`: `parking_photo` gains `uploader_id`, `thumbnail_key`, `width`, `height`, `processed_at`, `rejection_reason`, `reviewed_by`, `reviewed_at`; `moderation_state` default flips `APPROVED → PENDING_REVIEW`; queue + uploader indexes |
| Domain | `PhotoModerationState { PendingReview, Approved, Rejected }`; upload constraints (max bytes, max megapixels, input-format allowlist, output format/quality) |
| Application | `ImageProcessor` seam; `PhotoRepository` port; `PhotoService` use cases (`UploadPhoto`, `ApprovePhoto`, `RejectPhoto`, `ListPendingPhotos`); photo-upload rate-limit defaults |
| Infrastructure | `image`-crate `ImageProcessor` (decode → apply EXIF orientation → re-encode JPEG → thumbnail); `SqlxPhotoRepository`; extend `SqlxParkingPhotoReader` to return thumbnails |
| Web | multipart upload on P3 (`/parking/{id}/photo`) + optional D1 attach; `/moderation/photos` queue + approve/reject; gallery uses thumbnails; HTMX interactions; i18n additions |
| Cargo | `image` (jpeg/png/webp, default-features off) in infra; `axum` `multipart` feature in web |

### Explicitly out of scope (deferred, with where it lands)

| Item | Lands in |
|---|---|
| **D3 review-photo attachment** (§38 "optionally upload photos") | M5 — review moderation states land there (§44 "hide inappropriate reviews"); it reuses this pipeline but needs a `review_photo` table + review-card gallery, which belongs with review moderation |
| **Report-an-inappropriate-photo** button (§80) | M5 (reports table + report form are M5) |
| Hide/re-hide an already-APPROVED photo (§44) | M5 (general moderation actions) |
| Auto-detection of faces/license plates (§80 "MAY be introduced later") | not required for initial release |
| Real S3 storage; real `MEDIA_SIGNING_SECRET` | M7 (Ledger #7/#14) |
| Shared/Redis rate limiter; configurable upload limits | M7 (Ledger #6/#18) |
| Purge of rejected/abandoned upload objects | M6 retention jobs (M4 deletes bytes eagerly on reject, so only abandoned-in-flight objects remain) |

---

## 2. Decisions

| Decision | Choice | Reasoning |
|---|---|---|
| **Upload gate** | **Verified** (`require_verified`), same as add/edit/review/verify | §30 says "authenticated"; §16/§35 gate dataset contributions behind email verification. A photo is a dataset contribution and rides D1/P3 contributor actions, which are already verified-gated. Uniform gate, no new exemption |
| **Original upload handling** | **Discard the original immediately after processing.** Store only the re-encoded full derivative + thumbnail; never write the raw bytes to object storage | §80 "remove EXIF… avoid publishing original files where unnecessary"; §30 "the publicly served asset MUST be a processed derivative". Dropping the original eliminates the whole "original leaked / original retention" class of risk and needs no retention job. If a future need for originals arises (e.g. moderator deep-dive), revisit as a new privacy-sensitive decision |
| **Output format** | Re-encode all inputs to **JPEG** (quality 85) + JPEG thumbnail (max 400 px longest side) | §30 allowlist covers JPEG/PNG/WebP *input*; JPEG output is universally decodable and pure-Rust in the `image` crate (WebP *encoding* needs an extra dep). Re-encoding inherently strips EXIF/ICC/XMP and neutralizes polyglot/malformed-file tricks |
| **EXIF orientation** | **Apply EXIF Orientation during decode, then strip all metadata** | Modern phone JPEGs carry Orientation; stripping EXIF without applying it leaves rotated images. Read the tag, transform pixels, then re-encode with no metadata |
| **Key scheme** | `uploads/{photo_id}/full.jpg` + `uploads/{photo_id}/thumb.jpg`; `storage_key` (existing column) = full derivative, new `thumbnail_key` = thumbnail | Keeps the M1 gallery reader's `storage_key` semantics unchanged; seeded photos (which have no thumbnail) fall back to `storage_key`. `photo_id` comes from an insert-then-write compensating sequence (see §6) |
| **Upload validation** | Content sniff via magic bytes (never extension); allowlist JPEG/PNG/WebP; hard limits **10 MiB** file size and **20 megapixels** (§30 defaults); reject before full decode using the image header, then a decoded-byte budget | §30 "validated by actual file content, not merely filename extension" + "MUST define maximum upload size and supported formats" |
| **Moderation lifecycle** | `PENDING_REVIEW → APPROVED/REJECTED`, enforced by (a) the reader already filtering `APPROVED`, (b) the flipped column default, (c) `/media` requiring a valid signature so a guessed key isn't public | §30/§116.2. Approve sets `position = max(position)+1` for the location; reject sets `REJECTED` + `rejection_reason` and deletes both stored derivatives (idempotent `ObjectStorage::delete`) |
| **Queue visibility** | Moderators see the **processed derivative** (same bytes that would publish), linked location, `alt` caption, dimensions, and an anonymized "Contributor #id" label — never email/OAuth id | §80 "avoid exposing uploader email or OAuth identifiers". Reviewing the derivative (not an original that no longer exists) also confirms exactly what users would see |
| **Rate limiting (§45)** | Reuse the `RateLimiter` port (Ledger #6). Defaults: photo upload **10/day/user + 20/day/IP**. Approve/reject not rate-limited (moderators, audited) | §45 mandates photo-upload limits; defaults chosen now, tuned in M7 |
| **Audit (§44)** | `photo.uploaded` (success/failure), `photo.approved`, `photo.rejected` via the existing `AuditLog`; reject carries `rejection_reason` in metadata (no PII) | §44 "moderation actions MUST create audit events" |
| **D1 photo-attach** | D1 (`/parking/new`) switches to **multipart** and accepts one optional photo processed by the same pipeline; location is created first (transaction), then the photo references it | Honors PLAN M4 "D1 photo-attach". The P3 add-photo endpoint is the primary path; D1 attach is a thin wrapper over the same `PhotoService::upload` |
| **Compile-time SQL** | Continue `query_as!`/`query!` for all new readers/writers | §9/§305, established M1–M3 |
| **Input safety (§103)** | `alt` caption is trimmed/length-limited (≤ 500 chars) in the domain, escaped by Askama on render (never `|safe`) | §103 — UGC is untrusted |

---

## 3. Schema

### `migrations/0009_photos.sql`

```sql
-- §30/§116.2: real photo pipeline. M1 seeded pre-APPROVED originals; M4 adds
-- the upload→moderate columns and flips the default so new uploads are held
-- for review. `storage_key` is now the *full processed derivative*; the raw
-- upload is never stored (§80).

ALTER TABLE parking_photo
    ADD COLUMN uploader_id      BIGINT REFERENCES users(id) ON DELETE SET NULL,
    ADD COLUMN thumbnail_key    TEXT,                 -- processed thumbnail derivative
    ADD COLUMN width            INTEGER,              -- derivative pixel dimensions
    ADD COLUMN height           INTEGER,
    ADD COLUMN processed_at     TIMESTAMPTZ,          -- set when derivatives are stored
    ADD COLUMN rejection_reason TEXT,                 -- set by moderator on reject
    ADD COLUMN reviewed_by      BIGINT REFERENCES users(id) ON DELETE SET NULL,
    ADD COLUMN reviewed_at      TIMESTAMPTZ,
    ALTER COLUMN moderation_state SET DEFAULT 'PENDING_REVIEW';

-- Photo moderation queue (M2 screen), oldest first.
CREATE INDEX parking_photo_pending_idx
    ON parking_photo (moderation_state, created_at)
    WHERE moderation_state = 'PENDING_REVIEW';

-- Internal attribution / contributor history joins (never rendered publicly, §80).
CREATE INDEX parking_photo_uploader_idx
    ON parking_photo (uploader_id, created_at DESC);
```

Notes:

- Seeded rows keep `moderation_state = 'APPROVED'` because `seed.rs` passes it **explicitly** —
  the new default only affects future inserts.
- No `original_key` column exists because the original is discarded (§2). If that decision is
  revisited, this migration is the place an `original_key` would be added.
- `thumbnail_key` is NULL for M1 seeds; the reader/gallery fall back to `storage_key`.

---

## 4. Domain model (crates/domain)

New module `crates/domain/src/photo.rs` (pure, no I/O):

```
PhotoModerationState { PendingReview, Approved, Rejected }   // as_code/from_code
PhotoDimensions { width: u32, height: u32 }
```

Upload constraints (constants, source of truth for validation and error messages):

```rust
pub const MAX_PHOTO_BYTES: usize = 10 * 1024 * 1024;      // 10 MiB (§30)
pub const MAX_PHOTO_MEGAPIXELS: u64 = 20;                  // 20 MP (§30)
pub const THUMBNAIL_MAX_SIDE: u32 = 400;                   // derivative policy
pub const ALLOWED_INPUT_FORMATS: &[&str] = &["jpeg", "png", "webp"];  // content sniffed, not extension
pub const DERIVATIVE_QUALITY: u8 = 85;                     // JPEG output
```

Domain unit tests: `PhotoModerationState` code round-trips; `PhotoDimensions`/constraint helpers
(size-over-limit, megapixel-over-limit, format allowlist) at the boundaries.

---

## 5. Application layer (crates/application)

New module `crates/application/src/photo.rs`.

### Ports

```rust
// Internal processing seam (NOT a provider boundary — our own deterministic
// decode/re-encode logic). Trait-ified so application tests use a fast fake.
#[async_trait] trait ImageProcessor: Send + Sync {
    /// Decode → apply EXIF orientation → re-encode JPEG → thumbnail. Returns
    /// the two byte buffers + derivative dimensions + detected content type.
    async fn process(&self, bytes: &[u8]) -> Result<ProcessedImage, PhotoError>;
}
struct ProcessedImage {
    full: Vec<u8>,           // JPEG, EXIF-stripped, orientation applied
    thumb: Vec<u8>,          // JPEG, <= THUMBNAIL_MAX_SIDE
    width: u32, height: u32, // full-derivative dimensions
    content_type: &'static str, // "image/jpeg"
}

#[async_trait] trait PhotoRepository: Send + Sync {
    async fn insert_pending(&self, p: NewPendingPhoto) -> Result<i64, PhotoError>; // id
    async fn max_position(&self, location_id: i64) -> Result<i32, PhotoError>;
    async fn approve(&self, id: i64, moderator: UserId, position: i32) -> Result<(), PhotoError>;
    async fn reject(&self, id: i64, moderator: UserId, reason: &str) -> Result<RejectedPhoto, PhotoError>; // returns keys to delete
    async fn list_pending(&self) -> Result<Vec<PendingPhoto>, PhotoError>;
    async fn get_for_moderation(&self, id: i64) -> Result<Option<PhotoForModeration>, PhotoError>; // keys + state
}
```

`StoredPhoto` (in `ports.rs`) gains `thumbnail_key: Option<String>` so the gallery can prefer the
thumbnail; `SqlxParkingPhotoReader` returns it and the web layer picks.

### Use cases (`PhotoService`)

| Use case | Flow (abridged) |
|---|---|
| `UploadPhoto` | `require_verified` (web layer) → rate-limit (`photo:upload:user:{id}` and `photo:upload:ip:{ip}`) → check byte length ≤ `MAX_PHOTO_BYTES` → `ImageProcessor.process` (sniffs format, applies orientation, re-encodes, thumbnails; rejects >20 MP / non-allowlist / undecodable) → `insert_pending` (PENDING_REVIEW) → derive keys from the returned id → `storage.put(full)` + `storage.put(thumb)` → on storage failure, delete the row (compensate) → update `processed_at`/dimensions → audit `photo.uploaded` |
| `ApprovePhoto` | `require_role(Moderator)` → load state (must be `PENDING_REVIEW`, else idempotent no-op/error) → `max_position` + 1 → `approve` → audit `photo.approved` |
| `RejectPhoto` | `require_role(Moderator)` → `reject(reason)` (state → REJECTED, record reason/reviewer) → `storage.delete(full)` + `storage.delete(thumb)` (best-effort, logged) → audit `photo.rejected` |
| `ListPendingPhotos` | `require_role(Moderator)` → `list_pending` → web layer resolves presigned URLs (same `view::resolve_photo` mechanism) |

`PhotoError` variants: `NotVerified`, `RateLimited`, `TooLarge`, `UnsupportedFormat`,
`Undecodable`, `TooManyPixels`, `NotFound`, `NotPending`, `Unauthorized`, `Storage(StorageError)`,
`Internal`. Mapped by the web layer to friendly, non-leaking messages.

Rate-limit defaults (constants in `photo.rs`): `photo:upload:user:{id}` 10/day; `photo:upload:ip:{ip}`
20/day.

---

## 6. Infrastructure (crates/infrastructure)

- `photo/processor.rs` — `ImageProcessor` via the `image` crate (features `jpeg`, `png`, `webp`,
  default-features off): `ImageReader::into_dimensions` first (cheap header check for the 20 MP cap),
  then decode with format allowlist, read + apply EXIF `Orientation` (rotate/flip) before encoding,
  `save_buffer` JPEG quality 85 for the full, `thumbnail(THUMBNAIL_MAX_SIDE)` for the thumb.
- `photo/repository.rs` — `SqlxPhotoRepository` (`query_as!`): `insert_pending`, `max_position`,
  `approve` (state flip + position, one tx), `reject` (state flip + reason + reviewer, returns the
  two keys for the service to delete), `list_pending` (ordered `created_at`), `get_for_moderation`.
- `parking/photos.rs` — extend `SqlxParkingPhotoReader` to select `thumbnail_key` (and keep
  `storage_key`/`content_type`/`alt`); `StoredPhoto` carries it through.

`test-support` additions: `PendingPhotoBuilder` / extend `ParkingBuilder` with photos; reuse the
transaction/SAVEPOINT/committed-fixture harness from M1–M3. Add a tiny **JPEG fixture with EXIF
orientation** (checked into `test-support` or `web/static` test data) for the processor tests.

---

## 7. Web layer (crates/web)

### Middleware / gates

- Upload POSTs use `require_verified`; moderation routes use `require_role(Role::Moderator)`.
- **CSRF on multipart:** the auth middleware reads CSRF from `X-CSRF-Token` (htmx reads the
  `<meta name="csrf">`) or the `csrf` form field; multipart bodies do **not** carry the urlencoded
  field, so multipart forms send the header. Enforce an upload body size limit before streaming.
- Rate limiting applied inside `PhotoService` (per §5).

### Routes

| Route | Method | Page/action | Access |
|---|---|---|---|
| `/parking/{id}/photo` | POST | upload a photo (multipart: `photo` file + optional `alt`) | authenticated + verified |
| `/moderation/photos` | GET | M2 photo moderation queue | MODERATOR, ADMIN |
| `/moderation/photos/{id}/approve` | POST | approve a pending photo (HTMX) | MODERATOR, ADMIN |
| `/moderation/photos/{id}/reject` | POST | reject with reason (HTMX) | MODERATOR, ADMIN |

- `/parking/new` (D1) becomes a **multipart** form accepting one optional `photo`; the handler
  creates the location first, then calls `PhotoService::upload` for the attached file (same pipeline,
  same validation). On photo failure after create, the location persists and the page shows a
  "photo skipped" notice rather than failing the whole submit.
- `/parking/{id}` (P3): gallery `<img>` uses `thumbnail_key` (fallback `storage_key`), lightbox uses
  the full derivative; verified users see an "Add photo" control (file input + `hx-post` upload,
  swap-safe fragment on success/error per §116.6).

### Templates / i18n

- New page: `pages/moderation_photos.html` (grid/list of pending photos with approve/reject +
  reason input, linked location, "Contributor #id", dimensions, `alt`, "EXIF stripped" note).
- New partials: `photo_queue_item`, `photo_upload_form`, `photo_upload_result`.
- P3 additions: "Add photo" control; gallery thumbnails + lightbox full.
- D1 additions: optional photo file input; the create handler's multipart parsing.
- **i18n additions** (`crates/web/src/i18n.rs`): full en/pt-BR for upload labels, validation errors
  (too large / wrong format / not an image / too many pixels), moderation queue labels
  (pending/approve/reject/reason), success/failure toasts, "Contributor #" label. Strings stay in
  the web catalog (§12/§102).
- `m2-photos.html` is the visual contract for the queue; Tailwind utilities against the M0 `@theme`
  tokens, matching M1–M3. No inline `<script>` (Ledger #15).

---

## 8. Seeder / commands

No new commands. `seed-mock` keeps working: it passes `moderation_state = 'APPROVED'` explicitly, so
the flipped default doesn't affect it, and seeded rows simply have NULL `thumbnail_key` (gallery
falls back to `storage_key`). The bundled images are curated design assets pushed through the storage
port — this remains **mock data (Ledger #1/#7)**, not the production pipeline; production has no
seeds (§116.1). Optionally route seed photos through the processor so dev data matches the real
derivative shape — if done, note it under Ledger #1.

---

## 9. Testing

| Layer | Tests |
|---|---|
| domain | `PhotoModerationState` round-trips; size/megapixel/format boundary helpers |
| application | `UploadPhoto` (verified gate, rate-limit, too-large, unsupported format, undecodable, >20 MP, happy path → PENDING + audit); `ApprovePhoto`/`RejectPhoto` (role gate, non-pending idempotence, audit, reject deletes both keys); with fake processor/repo/storage |
| infrastructure (`#[db_test]`) | processor round-trip: JPEG-with-EXIF fixture → output has no EXIF/APP1 markers, orientation applied (dimensions swap), thumbnail ≤ 400 px; repo insert/approve/reject/position; queue ordering (oldest first); reader returns `thumbnail_key` |
| web (`#[db_test]`) | verified upload → row `PENDING_REVIEW`, **not** in gallery; anonymous/unverified → blocked; moderator approve → appears in gallery; reject → gone + storage objects deleted; `/media` on the derivative serves bytes with **no EXIF markers**; original filename/key never resolvable; moderation routes 403 for non-moderators; multipart CSRF (missing header → 403); rate-limit → 429; uploader email/OAuth id never appears in rendered queue HTML |
| security (§60) | authorization-boundary tests: upload + queue + approve/reject each require the right gate; reject is idempotent-safe (can't delete an already-deleted object); no uploader identity leaks into any rendered page |

---

## 10. Task breakdown

1. `0009_photos.sql`; verify `cargo run` applies it; confirm `seed-mock` still seeds APPROVED photos.
2. Domain: `photo.rs` (state enum + constraints) + unit tests.
3. Application: `photo.rs` (ports + `PhotoService` + rate-limit defaults + `PhotoError`);
   `StoredPhoto` gains `thumbnail_key`; tests with fakes. (`cargo add image`, `cargo add` only.)
4. Infrastructure: `ImageProcessor` (`image` crate); `SqlxPhotoRepository`; extend
   `SqlxParkingPhotoReader`; `test-support` builders + EXIF fixture; `#[db_test]` integration tests.
5. Web: multipart upload handler + P3 add-photo control; D1 multipart conversion; moderation queue
   page + approve/reject handlers; gallery thumbnail/lightbox; i18n additions; Tailwind classes
   matching `m2-photos.html`/P3.
6. HTTP/security tests; README (upload flow, moderation queue, env/config notes if any); Ledger
   entries; live acceptance walkthrough against `docker compose` + a registered+verified user and a
   seeded moderator.

## 11. Risks / notes

- **Decompression bombs** — cap by header dimensions (20 MP) *and* a decoded-byte budget; reject
  before full decode. A 20 MP image is ~60 MB raw, so don't hold several in memory at once.
- **EXIF orientation correctness** — the strip-without-apply bug is the classic failure; the
  fixture-with-orientation test is the guard.
- **Multipart CSRF** — multipart bodies can't carry the urlencoded `csrf` field; the header token is
  mandatory. A missing header must 403 (already the middleware's default).
- **Insert-then-write compensation** — if a storage `put` fails after the DB insert, delete the row
  (and any written object) so the queue never shows a half-written photo. Log the failure.
- **htmx 4 4xx-swap** — upload/approve/reject error responses must be swap-safe fragments
  (§116.6), reusing M2/M3's error-partial approach.
- **CSP** — file inputs and the existing gallery lightbox need no new inline `<script>`; keep it that
  way (Ledger #15).
- **Identity leaks** — `uploader_id` must never surface as email/OAuth subject; render
  "Contributor #id" only. Add an explicit HTML-absence assertion (§80).
- **D1 multipart conversion is the riskiest web change** — if it proves heavy, fall back to the
  create-then-upload-via-P3 flow (the primary path) and defer D1 attach; the pipeline is identical.
- **Eager byte deletion on reject** — orphaned objects are impossible for rejected photos; the only
  residual is uploads interrupted mid-flight, covered by M6 retention.
- **`image` crate build** — jpeg/png/webp decode is pure Rust, but confirm the `webp` decode feature
  doesn't pull a system lib at implementation; if it does, drop WebP input and keep JPEG/PNG.

---

## Ledger additions this milestone

| # | Item | Kind | Introduced | Remove/improve by | Notes |
|---|---|---|---|---|---|
| 18 | Photo upload/processing constants hardcoded (10 MiB, 20 MP, JPEG q85, 400 px thumb, rate-limit defaults) | improve | M4 | M7 | Make size/dimension/quality/limits configurable; document for tuning |

Also updated: **Ledger #6** now covers photo-upload limits in addition to M2 auth and M3
contribution limits.
