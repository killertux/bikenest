# M2 — Accounts & authentication — implementation plan

> **Status: implemented.** Derived from `PLAN.md` (M2) and `REQUIREMENTS.md`
> (§16–§20, §45, §47, §60, §64–§65, §67–§68, §78, §100). Parent plan: `PLAN.md`.

Companion to `REQUIREMENTS.md` (§16–§20 drive this milestone), `PLAN.md` (M2 overview) and
`UI_DESIGN.md` + `design-project/` screens `a1-register.html`, `a2-login.html`,
`a3-verify-email.html`, `a4-reset-password.html`, `a5-reset-password.html`, `c1-account.html`,
`c2-password.html`, `c3-email.html`, `m5-users.html` (visual contract).

**Goal:** real accounts, secure sessions, and the role model. Register → verify → log in/out;
seeded admin promotes a user; sessions + CSRF; deny-by-default authorization on the roles.

**Working app means (acceptance):** register an email/password account (captured verification
email) → follow the link → the account becomes verified → log in → land on `/account` with an
authenticated nav → log out → log in again. A seeded admin (`seed-admin`) opens `/admin/users`,
grants MODERATOR to another account, and the change is audited. Suspended accounts are blocked at
login; unverified accounts see the "verify your email to contribute" banner. Repeated failed logins
and repeated reset requests are throttled with no account-existence leak. `cargo test` green;
fresh-clone onboarding from README still works.

---

## 1. Scope

### In scope

| Area | Content |
|---|---|
| Schema | `0005_accounts.sql`: extend `users`; `authentication_identities`, `sessions`, `email_verification_tokens`, `password_reset_tokens`, `user_roles`. `0006_audit_events.sql`: `audit_events` |
| Domain | `AccountState` lifecycle, `Role`, `AuthenticationProvider` (provider kind), `Password`/`PasswordPolicy`, `SessionId`/`CsrfToken`/`VerificationToken` value objects |
| Application | `Auth` port cluster (`PasswordHasher`, `TokenGenerator`, `Clock`); use cases: `RegisterAccount`, `VerifyEmail`, `ResendVerification`, `Login`, `Logout`, `RequestPasswordReset`, `ResetPassword`, `ChangePassword`, `ChangeEmail`, `GrantRole`/`RevokeRole`, `OAuthCallback`; `RateLimiter` port; `EmailProvider` port; `AuthenticationProvider` (OAuth) port; `AuditLog` port |
| Infrastructure | `SqlxAccountRepository`, `SqlxSessionStore`, `SqlxTokenStore`, `SqlxAuditLog` (compile-time `query_as!`), `Argon2PasswordHasher`, `RealTokenGenerator`, `SystemClock`, `InMemoryRateLimiter`, `FakeEmailProvider`, `FakeOAuthProvider` |
| Web | Routes for A1–A6, C1–C3, M5 (see §7); session/CSRF/authz middleware; authenticated nav + account menu; i18n catalog additions |
| Commands | `seed-admin` subcommand (env-driven, idempotent) — **Ledger #10** |

### Explicitly out of scope (deferred, with where it lands)

| Item | Lands in |
|---|---|
| Contribution actions gated on verification (add/review/verify/favorite/photo) | M3/M4/M5 (the *gate* — an `is_verified` flag on the principal — ships in M2) |
| Suspend/restore actions + moderation user management | M5 (the `SUSPENDED` state and its login block ship in M2) |
| Account deletion/anonymization + export | M6 (`DELETED` is defined in the enum now; the transition is M6) |
| Real Google OAuth credentials | M7 (Ledger #5) |
| Real email provider (SMTP/ESP) | M7 (Ledger #4) |
| Shared/Redis-backed rate limiter | M7 (Ledger #6) |
| Security headers (CSP, HSTS, …) | M7 (Ledger #15); M2 adds only the cookie flags + CSRF |
| Contribution/review/report/photo rate limits | M3/M4 (reuse the `RateLimiter` port from M2) |
| Admin audit-log viewer page (M6) | M5/M6 (the `audit_events` table + writer ship in M2) |
| Privacy policy / cookie inventory pages | M6 (P4/P5/P6; minimal placeholder only if legally required for session cookies) |

---

## 2. Decisions

| Decision | Choice | Reasoning |
|---|---|---|
| Password hashing | **argon2id** via the `argon2` crate (password-hash API), OWASP params (m=19456 KiB, t=2, p=1); per-hash random salt encoded in the PHC string; verify with constant-time comparison | §16 "modern password hashing algorithm appropriate for password storage"; argon2id is the memory-hard recommendation for interactive logins |
| Password policy | Minimum length **8**, no forced character classes; guidance text only (UI_DESIGN A1 "password strength guidance") | §16 has no complexity mandate; length is the highest-signal rule and avoids NIST-discouraged composition rules |
| User ↔ identity model | `users` = account (canonical email, state, verified-at); `authentication_identities` = one row per login method: `provider='password'` (subject = `lower(email)`, holds the argon2id `credential_hash`) and `provider='google'` (subject = Google `sub`, no credential). `UNIQUE(provider, provider_subject)` | Faithful to §17's `User ↔ AuthenticationIdentity` split; adding a future provider is a pure insert |
| Identity privacy | No endpoint ever returns `provider_subject` or `credential_hash`; the account page shows only the provider *kind* ("Email"/"Google") | §17 "identities MUST NOT be publicly exposed" |
| Canonical email + password subject sync | `users.email` stays the canonical contact address; the `password` identity's `provider_subject` is kept equal to it, updated in the same transaction as any email change | Single "one account per email" invariant (`idx_users_email`) and one login-lookup key, never divergent |
| Account lifecycle | `AccountState` enum: `PendingEmailVerification → Active → Suspended → Deleted`, with per-state capabilities (table in §4). M2 implements `PendingEmailVerification → Active` (via verification) and *enforces* `Suspended`/`Deleted` at login; the suspend/delete *transitions* are M5/M6 | §20 requires the lifecycle defined and what each state can do |
| Verification state | `users.email_verified_at TIMESTAMPTZ` (set on the *canonical* email's verification); the "can contribute" gate is `email_verified_at IS NOT NULL`, exposed on the session principal as `is_verified` | §16's unverified-account policy; the flag is cheap to compute once per session |
| Email verification tokens | 256-bit random (URL-safe, base64url), stored **only** as SHA-256 hash; `expires_at = now + 24h`; single-use (`used_at`); row records the **email being verified** so the same table serves registration and change-email | §16: random, expiring, single-use, invalidated on use, not plaintext |
| Password reset tokens | Same token design (256-bit, hashed, `expires_at = now + 1h`, single-use); resetting **revokes all sessions** (keep the current one) | §16/§18: password/security events invalidate sessions |
| Sessions | Server-side: 256-bit random session id → cookie holds the raw id, DB holds **SHA-256 hash** (`sessions.token_hash` PK). Cookie `HttpOnly; Secure; SameSite=Lax; Path=/`. Sliding idle expiry 30 days (`last_seen_at` refreshed), absolute cap 90 days | §18/§78; opaque, unpredictable, contains no user info; hashing at rest limits blast radius of a DB read |
| Session lookup | Every authenticated request resolves the cookie → hash → row (joined to `users` + roles); a miss/invalid/expired/revoked row clears the cookie and treats the request as anonymous | Deny-by-default: no session ⇒ no principal |
| CSRF | Synchronizer-token: 32 random bytes stored **in the session row**, rendered as a hidden form input and sent on HTMX requests via a `<meta name="csrf">` + `htmx:configRequest` header (`X-CSRF-Token`); validated constant-time on all state-changing routes | §18; token is server-side-only and scoped to the session |
| CSRF scope | Enforced on **every non-GET** route that mutates state or authenticates; GET stays side-effect-free (§108 API philosophy) | Simple, uniform rule |
| Authorization | Deny-by-default: middleware attaches an anonymous/authenticated principal; **application-layer** role checks (`require_role(Role::Admin)`) gate handlers; UI hides links but is never the enforcement point (§19) | §19 "enforced in the application layer rather than relying on UI visibility" |
| Roles storage | `user_roles(user_id, role, granted_by, granted_at)`, `role` as validated `TEXT`, `PK(user_id, role)`. `USER` is granted at registration (implicit baseline); `MODERATOR`/`ADMIN` granted via audited use cases | §26-style extensibility without migration; explicit grants are auditable and revocable |
| Role changes | `GrantRole`/`RevokeRole` require an `ADMIN` principal, are denied by default, write an audit event, and cannot run through self-service settings; revoking one's *own last* `ADMIN` is refused (prevents lockout) | §19/§47; lockout guard is a pragmatic invariant |
| Seed admin | `seed-admin` subcommand reads `ADMIN_EMAIL`/`ADMIN_PASSWORD` from env, is idempotent (upsert identity, ensure `ACTIVE` + verified + `USER`+`ADMIN` roles), never reachable via HTTP | §19/§116.3; Ledger #10 |
| Rate limiting | `RateLimiter` port; `InMemoryRateLimiter` (sliding-window counter per key, `Mutex<HashMap>`). Defaults below; keys are per-IP and per-identifier (email). Responses are **identical whether or not the account exists** | §45; Ledger #6 |
| Rate-limit defaults | login 5/15 min per `ip+email` + 10/15 min per IP; register 3/h per IP; reset-request 3/h per `ip` + 3/h per email; verify-resend 3/h per user + 5/h per IP | §45 chosen defaults, configurable; documented for tuning in M7 |
| No-existence-leak rule | Login failure, registration of an existing email, and reset-request all return the same generic message (register: "check your inbox" even if the email is taken; reset: "if that address exists…") | §45 + UI_DESIGN A1/A2/A4 |
| Email provider | `EmailProvider` port (`send_verification`, `send_password_reset`); `FakeEmailProvider` writes a `.eml`-style capture to `<MEDIA_ROOT>/outbox/` **and** logs the link to stdout (dev) | §84 provider abstraction; Ledger #4 |
| OAuth | `AuthenticationProvider` port (`authorize_url`, `exchange(code) -> ProviderIdentity { provider, subject, email, email_verified }`); `FakeOAuthProvider` serves dev-only `/auth/google` + `/auth/google/callback` flow with a deterministic identity (no Google credentials) | §16/§17; Ledger #5 |
| OAuth account linking | On callback: match by `(provider, subject)` → log in; else match by verified `email` → **link** a new identity to the existing account; else create account (state `Active` if the provider asserts a verified email, else `PendingEmailVerification`) + link | §16 "maps to the same internal user model"; prevents duplicate accounts |
| Audit events | `audit_events` table + `AuditLog` port (`record(actor, action, target, result, metadata)`); written for: login success/failure, logout, registration, email verification, password change, email-change request/confirm, role grant/revoke, admin seed. No passwords/tokens/PII beyond actor/target ids | §47 |
| Clock | `Clock` port (`now()`) with `SystemClock` impl; injected into token/session/rate-limit expiry paths | Deterministic expiry tests without time-mocking |
| Token generation | `TokenGenerator` port (`generate(n_bytes) -> [u8; 32]` via `rand`/OsRng) with a real impl only | §16 cryptographic randomness; trivial real impl, no fake |
| Compile-time SQL | Continue `query_as!`/`query!` macros for all new readers/writers (`.env` at compile time already documented in README) | §9/§305, established M1 |
| Rate-limiter storage | In-memory only in M2 (single-instance dev); the port shape (`check(key, limit, window)`) is chosen so a Redis-backed impl is a drop-in in M7 | §45/§84; Ledger #6 |

---

## 3. Schema

### `migrations/0005_accounts.sql`

```sql
-- Extend the M0 users table with the account lifecycle (§20).
ALTER TABLE users
    ADD COLUMN account_state    TEXT NOT NULL DEFAULT 'PENDING_EMAIL_VERIFICATION',
    ADD COLUMN email_verified_at TIMESTAMPTZ,
    ADD COLUMN suspended_at     TIMESTAMPTZ;
-- account_state values: 'PENDING_EMAIL_VERIFICATION' | 'ACTIVE' | 'SUSPENDED' | 'DELETED'
-- (validated in the domain; TEXT keeps it extensible like parking_type).

-- One row per login method (§17): password and google now, future providers later.
CREATE TABLE authentication_identities (
    id               BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id          BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider         TEXT NOT NULL,              -- 'password' | 'google'
    provider_subject TEXT NOT NULL,              -- password → lower(email); google → `sub`
    credential_hash  TEXT,                       -- argon2id PHC string; NULL for OAuth
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (provider, provider_subject)
);
CREATE INDEX authentication_identities_user_idx ON authentication_identities (user_id);

-- Server-side sessions (§18). The cookie holds the raw id; the DB stores its
-- SHA-256 hash only. `csrf_token` is the per-session synchronizer token.
CREATE TABLE sessions (
    token_hash   TEXT PRIMARY KEY,               -- sha256_hex(raw session id)
    user_id      BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    csrf_token   TEXT NOT NULL,                  -- 32 random bytes, base64url (server-only)
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at   TIMESTAMPTZ NOT NULL,           -- absolute cap (90 days from creation)
    revoked_at   TIMESTAMPTZ
);
CREATE INDEX sessions_user_idx ON sessions (user_id);
CREATE INDEX sessions_expires_idx ON sessions (expires_at);

-- Email verification tokens (§16): registration + change-email share this table.
CREATE TABLE email_verification_tokens (
    token_hash TEXT PRIMARY KEY,                 -- sha256_hex(raw token)
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    email      TEXT NOT NULL,                    -- the address being verified
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,             -- now + 24h
    used_at    TIMESTAMPTZ
);
CREATE INDEX email_verification_tokens_user_idx ON email_verification_tokens (user_id);

-- Password reset tokens (§16): single-use, short-lived.
CREATE TABLE password_reset_tokens (
    token_hash TEXT PRIMARY KEY,                 -- sha256_hex(raw token)
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,             -- now + 1h
    used_at    TIMESTAMPTZ
);
CREATE INDEX password_reset_tokens_user_idx ON password_reset_tokens (user_id);

-- Granted roles (§19). USER is implicit baseline (granted at registration).
CREATE TABLE user_roles (
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role       TEXT NOT NULL,                    -- 'USER' | 'MODERATOR' | 'ADMIN'
    granted_by BIGINT REFERENCES users(id) ON DELETE SET NULL,  -- NULL for bootstrap
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, role)
);
CREATE INDEX user_roles_role_idx ON user_roles (role);
```

### `migrations/0006_audit_events.sql`

```sql
-- §47: security, account and role actions. No tokens/passwords/PII beyond
-- actor/target identifiers. `metadata` is JSONB for per-action context.
CREATE TABLE audit_events (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    actor_user_id BIGINT REFERENCES users(id) ON DELETE SET NULL,  -- NULL = system
    action      TEXT NOT NULL,                  -- 'auth.login' | 'role.granted' | …
    target_type TEXT NOT NULL,                  -- 'user' | 'session' | 'role'
    target_id   TEXT NOT NULL,
    result      TEXT NOT NULL,                  -- 'success' | 'failure'
    metadata    JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX audit_events_actor_idx  ON audit_events (actor_user_id);
CREATE INDEX audit_events_action_idx ON audit_events (action, created_at DESC);
```

State-machine note (§20, defined now; the suspend/delete *transitions* are M5/M6):

```
PENDING_EMAIL_VERIFICATION → ACTIVE          (email verified — M2)
ACTIVE → SUSPENDED                           (moderation — M5)
SUSPENDED → ACTIVE                           (restore — M5)
ACTIVE | SUSPENDED → DELETED / ANONYMIZED    (deletion — M6)
```

---

## 4. Domain model (crates/domain)

New value objects / enums (all pure, no I/O):

```
AccountState { PendingEmailVerification, Active, Suspended, Deleted }   // as_code/from_code
Role { User, Moderator, Admin }                                          // as_code/from_code
AuthenticationProvider { Password, Google }                              // as_code/from_code
Password(String)          // never printed; validated by PasswordPolicy
PasswordPolicy { min_len: usize }                                        // validate(&str) -> Result<(), DomainError>
SessionId([u8; 32])       // raw session id (only the hash is persisted)
CsrfToken([u8; 32])
VerificationToken([u8; 32])  // + reset token — same shape, distinct types
```

`User` grows to `User { id, email, display_name, account_state, email_verified_at, roles: Vec<Role> }`.

Per-state capability table (drives the login gate now, contribution gate in M3):

| State | Log in | Browse/search | Account settings (C1–C3) | Contribute (M3+) | Notes |
|---|---|---|---|---|---|
| `PendingEmailVerification` | ✅ | ✅ | ✅ | ❌ (verified gate) | C1 "verify your email to contribute" banner |
| `Active` | ✅ | ✅ | ✅ | ✅ | |
| `Suspended` | ❌ (blocked at login) | ✅ (public) | ❌ | ❌ | generic "account disabled" on login |
| `Deleted` | ❌ | ✅ (public) | ❌ | ❌ | M6 anonymizes |

Domain unit tests: `AccountState`/`Role`/`AuthenticationProvider` code round-trips; `PasswordPolicy`
boundaries; `SessionId`/`VerificationToken` type separation (no accidental cross-use).

---

## 5. Application layer (crates/application)

New ports (`crates/application/src/auth.rs`, `rate_limit.rs`, `email.rs`, `audit.rs`):

```rust
#[async_trait] trait PasswordHasher { async fn hash(&self, pw: &Password) -> Result<String, AuthError>;
                                    async fn verify(&self, pw: &Password, hash: &str) -> Result<bool, AuthError>; }
#[async_trait] trait TokenGenerator { fn generate(&self) -> [u8; 32]; }            // sync, OsRng
#[async_trait] trait Clock { fn now(&self) -> DateTime<Utc>; }                     // sync

#[async_trait] trait AccountRepository {
    async fn find_by_email(&self, email: &UserEmail) -> Result<Option<Account>, ReaderError>;
    async fn find_by_id(&self, id: UserId) -> Result<Option<Account>, ReaderError>;
    async fn create(&self, new: NewAccount) -> Result<UserId, WriterError>;        // user + password identity + USER role
    async fn set_state(&self, id: UserId, state: AccountState) -> Result<(), WriterError>;
    async fn mark_email_verified(&self, id: UserId, at: DateTime<Utc>) -> Result<(), WriterError>;
    async fn update_canonical_email(&self, id: UserId, email: &UserEmail) -> Result<(), WriterError>;
    async fn set_password(&self, id: UserId, hash: &str) -> Result<(), WriterError>;
    async fn link_identity(&self, user_id: UserId, provider: AuthenticationProvider, subject: &str, hash: Option<&str>) -> Result<(), WriterError>;
    async fn find_identity(&self, provider: AuthenticationProvider, subject: &str) -> Result<Option<IdentityRecord>, ReaderError>;
    async fn roles(&self, id: UserId) -> Result<Vec<Role>, ReaderError>;
    async fn grant_role(&self, id: UserId, role: Role, by: UserId) -> Result<(), WriterError>;
    async fn revoke_role(&self, id: UserId, role: Role) -> Result<bool, WriterError>;  // false if absent
}

#[async_trait] trait SessionStore {
    async fn create(&self, user_id: UserId, raw: &SessionId, csrf: &CsrfToken, now: DateTime<Utc>) -> Result<(), WriterError>;
    async fn resolve(&self, raw: &SessionId, now: DateTime<Utc>) -> Result<Option<Session>, ReaderError>;  // hash+lookup+refresh last_seen
    async fn revoke(&self, raw: &SessionId) -> Result<(), WriterError>;
    async fn revoke_all_for_user_except(&self, user_id: UserId, keep: &SessionId) -> Result<(), WriterError>;
}

#[async_trait] trait TokenStore {
    async fn issue_verification(&self, user_id: UserId, email: &str, raw: &VerificationToken, now: DateTime<Utc>) -> Result<(), WriterError>;
    async fn consume_verification(&self, raw: &VerificationToken, now: DateTime<Utc>) -> Result<Option<(UserId, String)>, ReaderError>;  // (user_id, email)
    async fn issue_reset(&self, user_id: UserId, raw: &VerificationToken, now: DateTime<Utc>) -> Result<(), WriterError>;
    async fn consume_reset(&self, raw: &VerificationToken, now: DateTime<Utc>) -> Result<Option<UserId>, ReaderError>;
}

#[async_trait] trait EmailProvider { async fn send_verification(&self, to: &UserEmail, link: &str) -> Result<(), EmailError>;
                                    async fn send_password_reset(&self, to: &UserEmail, link: &str) -> Result<(), EmailError>; }

#[async_trait] trait AuthenticationProvider {                    // OAuth (Google)
    fn authorize_url(&self, state: &str) -> String;
    async fn exchange(&self, code: &str) -> Result<ProviderIdentity, AuthError>;
}

#[async_trait] trait RateLimiter { async fn check(&self, key: &str, limit: u32, window: Duration) -> Result<bool, RateLimitError>; } // false = over limit

#[async_trait] trait AuditLog { async fn record(&self, event: AuditEvent) -> Result<(), WriterError>; }
```

Use cases (each owns one clear step; handlers are thin):

| Use case | Flow (abridged) |
|---|---|
| `RegisterAccount` | rate-limit IP → validate email/password → if email taken, return the *same* success as a fresh signup (no-existence-leak) but send no email → else create user (state `PendingEmailVerification`, password identity, `USER` role) + verification token + email + audit |
| `VerifyEmail` | consume token (single-use, expiry) → `mark_email_verified` on `users.email` → set state `Active` (if still pending) → audit |
| `ResendVerification` | rate-limit → find user by email → (re)issue token + email; neutral response |
| `Login` | rate-limit ip+email → load identity by email → verify password (constant-time) → check `account_state ∈ {Active, PendingEmailVerification}` → create session + cookie → audit success/failure (same generic failure for bad creds *and* suspended/deleted) |
| `Logout` | revoke current session, clear cookie |
| `RequestPasswordReset` | rate-limit ip + email → issue token + email only if account exists; neutral response |
| `ResetPassword` | consume reset token → set new password hash → `revoke_all_for_user_except(current)` → audit |
| `ChangePassword` | (authed) verify current password → set new hash → revoke other sessions → audit |
| `ChangeEmail` | (authed) verify current password → issue a verification token for the *new* email → on `VerifyEmail` for that token, update `users.email` + password identity subject + `email_verified_at` in one transaction → revoke other sessions → audit |
| `OAuthCallback` | exchange code → match `(provider, subject)` → login; else match verified email → link; else create + link → session + cookie → audit |
| `GrantRole`/`RevokeRole` | require ADMIN principal → grant/revoke on target → audit (deny self-revoke of last ADMIN) |

Error type `AuthError` covers `InvalidCredentials`, `EmailTaken`, `TokenExpired`, `TokenUsed`,
`TokenInvalid`, `RateLimited`, `AccountSuspended`, `AccountDeleted`, `ProviderFailed`,
`WeakPassword`, etc. — all mapped by the web layer to generic, leak-free messages.

A `Principal { user: Option<AuthenticatedUser> }` read model carries `id`, `email`,
`account_state`, `is_verified`, `roles: Vec<Role>`; the web middleware resolves it from the session
and passes it to handlers. `AuthenticatedUser::has_role(Role)` is the single authorization check.

---

## 6. Infrastructure (crates/infrastructure)

- `auth/password.rs` — `Argon2PasswordHasher` (`argon2` crate, `Argon2::default()`); `hash`/`verify`
  behind the port.
- `auth/token.rs` — `RealTokenGenerator` (`rand::rngs::OsRng` + `getrandom`); `sha256_hex` helper
  shared with session/token stores.
- `auth/clock.rs` — `SystemClock` (`chrono::Utc::now`).
- `auth/account_repo.rs` — `SqlxAccountRepository` (compile-time `query_as!`): user+identity+role
  reads/writes; the multi-row mutations (register, change-email confirm) are single transactions.
- `auth/session_store.rs` — `SqlxSessionStore`: create/resolve/revoke; `resolve` updates
  `last_seen_at` (sliding idle expiry) and returns `None` past `expires_at`/`revoked_at`.
- `auth/token_store.rs` — `SqlxTokenStore`: issue/consume with `UPDATE … SET used_at = now()
  WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now() RETURNING …` (single-use is
  enforced atomically by the `used_at IS NULL` guard, not by read-then-write).
- `auth/rate_limit.rs` — `InMemoryRateLimiter` (sliding-window counters per key under a `Mutex`;
  cleans expired buckets on read). Ledger #6.
- `auth/email.rs` — `FakeEmailProvider`: appends a plain-text capture to
  `<MEDIA_ROOT>/outbox/<ts>-<kind>-<n>.txt` and `tracing::info!`s the link (dev convenience). Ledger #4.
- `auth/oauth.rs` — `FakeOAuthProvider`: deterministic dev identity from a configured
  `FAKE_OAUTH_EMAIL`/`FAKE_OAUTH_SUB` (defaults), no network. Ledger #5.
- `auth/audit.rs` — `SqlxAuditLog` (`query!` insert).
- `auth/seed.rs` — `seed_admin` command (idempotent upsert) using the repository, Ledger #10.

`test-support` additions: extend `UserBuilder` (account state, verified-at, password hash, roles),
and add `SessionBuilder`, `VerificationTokenBuilder`, `RoleBuilder` helpers; the existing
transaction/SAVEPOINT/committed-fixture harness is reused unchanged.

---

## 7. Web layer (crates/web)

### Middleware (new `crates/web/src/auth.rs`)

1. **Session resolver** — read cookie → hash → `SessionStore::resolve` → attach `Principal`
   (anonymous on miss/expiry/revoke; also clear an invalid cookie).
2. **CSRF** — for every non-GET route, compare `X-CSRF-Token` header (HTMX) or hidden `csrf` form
   field against the session's token, constant-time; mismatch → 403 (generic). GET requests are
   side-effect-free (§108).
3. **Rate limiting** — applied per-endpoint inside the auth handlers (not global), with
   per-IP + per-identifier keys.

### Routes

| Route | Method | Page/action | Access |
|---|---|---|---|
| `/register` | GET/POST | A1 register | public (redirect if authed) |
| `/login` | GET/POST | A2 login | public (redirect if authed) |
| `/logout` | POST | sign out | authenticated |
| `/verify-email` | GET | A3 confirm (`?token=…`) | public |
| `/verify-email/resend` | POST | A3 resend | public (rate-limited) |
| `/password-reset` | GET/POST | A4 request | public |
| `/password-reset` | POST | A5 set new (`?token=…` via hidden field) | public (token-gated) |
| `/auth/google` | GET | A6 initiate (`/auth/google/callback`) | public |
| `/auth/google/callback` | GET | A6 exchange + link | public |
| `/account` | GET | C1 overview | authenticated |
| `/account/password` | GET/POST | C2 change password | authenticated |
| `/account/email` | GET/POST | C3 change email | authenticated |
| `/admin/users` | GET | M5 user list | ADMIN |
| `/admin/users/{id}/role` | POST | M5 grant/revoke | ADMIN |

### Templates / i18n

- New `layouts/base.html` nav states: anonymous (Log in / Sign up — keys `auth.login`/`auth.signup`
  already exist) vs authenticated (account menu with `/account`, sign out).
- New pages: `pages/{register,login,verify_email,password_reset,password_reset_new,account,
  account_password,account_email,admin_users}.html` + partials for inline field errors and the
  unverified banner (C1). All forms render the CSRF hidden input; HTMX requests attach the header
  via `htmx:configRequest` reading `<meta name="csrf">`.
- **i18n catalog additions** (`crates/web/src/i18n.rs`): full en/pt-BR strings for A1–A6, C1–C3,
  M5 — field labels, validation errors, generic auth failures ("email or password incorrect",
  "if that address exists…"), the unverified banner, session-expired notice, and admin user-management
  labels. Strings stay in the web catalog, never in domain/application logic (§12/§102).
- The design screens `a1…a5`, `c1…c3`, `m5-users.html` are the visual contract; Tailwind utilities
  against the M0 `@theme` tokens, matching M1's approach.

---

## 8. Seeder / commands

`cargo run -p bikenest-web -- seed-admin` (dispatched in `main.rs` alongside `seed-mock`):

- Reads `ADMIN_EMAIL` / `ADMIN_PASSWORD` from env (error + exit if missing, with a hint).
- Idempotent: upsert user (or adopt an existing one), ensure `ACTIVE` + `email_verified_at` +
  password identity + `USER`/`ADMIN` roles; prints whether it created or updated.
- Never reachable over HTTP; writes an audit event (`admin.seeded`). Ledger #10.

---

## 9. Testing

| Layer | Tests |
|---|---|
| domain | `AccountState`/`Role`/`AuthenticationProvider` code round-trips; `PasswordPolicy` boundaries; token/session type separation |
| application | use cases with fake repo/session/token/email/oauth/rate-limit/clock/audit: register (email-taken leak-free path), verify (single-use + expiry + invalid), login (good creds, bad creds, suspended/deleted blocked — same message), reset (request is neutral; reset revokes sessions), change-email (old email valid until new is verified), grant/revoke (non-admin denied, self-revoke-of-last-admin refused), rate-limit enforcement |
| infrastructure (`#[db_test]`) | account repo round-trip (identity link/unlink, role grant/revoke); session store create/resolve/expire/revoke; token store single-use via the atomic `used_at` guard (concurrent consume); audit insert |
| web (`#[db_test]`) | register → verify (fake email captured) → login sets cookie → `/account` 200 → logout; login failure returns the same body as unknown-email; `/admin/users` 401/403 as anonymous and as non-admin, 200 as admin; role grant/revoke audit row written; CSRF: POST without token → 403; rate-limit: N failed logins → 429/generic and no account-existence leak; suspended user blocked at login; unverified banner present on C1 |
| security (§60) | authorization-boundary table-driven tests: unauthenticated → redirect/deny; non-admin → deny admin routes; suspended/deleted → cannot authenticate; no endpoint exposes `provider_subject`/`credential_hash` |

---

## 10. Task breakdown

1. `0005_accounts.sql` + `0006_audit_events.sql`; verify `cargo run` applies them.
2. Domain: `AccountState`, `Role`, `AuthenticationProvider`, `Password`/`PasswordPolicy`,
   `SessionId`/`CsrfToken`/`VerificationToken`, expand `User`; unit tests.
3. Application: auth/email/oauth/rate-limit/audit ports + `AuthError`; the use-case set + tests
   with fakes. (`cargo add argon2 rand chrono` where needed — `cargo add` only, §11.)
4. Infrastructure: `Argon2PasswordHasher`, `RealTokenGenerator`, `SystemClock`,
   `SqlxAccountRepository`, `SqlxSessionStore`, `SqlxTokenStore`, `SqlxAuditLog`,
   `InMemoryRateLimiter`, `FakeEmailProvider`, `FakeOAuthProvider`; extend `test-support` builders;
   `#[db_test]` integration tests.
5. `seed-admin` command + env vars in `.env.example`.
6. Web: session/CSRF/authz middleware; handlers + route wiring; templates/components/partials;
   authenticated nav + account menu; i18n catalog additions; Tailwind classes matching design
   screens.
7. HTTP/security tests; README (`seed-admin`, fake-email outbox, env vars, new commands); Ledger
   entries #4/#5/#6/#10; live acceptance walkthrough against `docker compose`.

## 11. Risks / notes

- **Fake OAuth is a stub with a narrow surface** — the real Google flow (PKCE, `state` replay
  protection, `nonce`) is only sketched in M2; the port's `exchange` signature must stay compatible
  with a real client in M7 (Ledger #5). Keep the stub's callback route dev-gated.
- **In-memory rate limiter is per-instance** — fine single-instance; documented as Ledger #6 for a
  Redis-backed store before multi-instance deployment.
- **Session/CSRF cookies and the future CSP** — `SameSite=Lax` + synchronizer token works without
  `unsafe-inline`/`unsafe-eval`, but the HTMX header approach must keep working when M7 enforces a
  strict CSP (Ledger #15). Avoid inline `<script>` in the new templates.
- **Token expiry uses wall-clock `now()`** — all expiry logic goes through the injected `Clock`
  (never `chrono::Utc::now()` inline) so tests stay deterministic.
- **Compile-time `query_as!` needs the DB up** — unchanged from M1; README already documents it.
- **No-existence-leak discipline is easy to regress** — the auth tests assert byte-identical bodies
  for the "exists" vs "doesn't exist" cases on login/register/reset (§45).

---

## Ledger additions this milestone

| # | Item | Kind | Introduced | Remove/improve by | Notes |
|---|---|---|---|---|---|
| 4 | `FakeEmailProvider` (capturing, outbox + stdout) | fake | M2 | M7 | Replace with SMTP/ESP; keep capture mode for tests |
| 5 | `FakeOAuthProvider` (Google stub, dev routes) | fake/stub | M2 | M7 | Replace with real Google OAuth client + credentials (PKCE/state/nonce) |
| 6 | In-memory `RateLimiter` | stub | M2 | M7 | Auth limits now; contribution limits M3. Replace with Redis-backed if multi-instance |
| 10 | `seed-admin` command | improve | M2 | M7 | Ensure idempotent + secret-safe in production |
