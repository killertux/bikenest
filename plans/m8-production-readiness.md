# M8 — Production readiness & config plumbing (deferred from M7)

> **Status: planned (deferred from M7).** M7's *security & robustness hardening* and its *SEO/i18n
> core* are shipped (see `plans/m7-hardening.md`). This milestone is the **production
> deployment/backups/incident-response docs + Dockerfile**, the **runtime plumbing** of the config
> knobs that M7 exposed but did not yet thread through, and the **i18n finish** (locale date
> formatting + string audit). Browser E2E / axe-core accessibility was **removed from scope** (see
> `plans/m7-hardening.md` — the static CSP/Alpine verification stands; a real-browser smoke is a
> follow-up if/when a QA harness is added). The real-provider cutover and production gating live in
> `PENDING_FOR_PRODUCTION.md` (§A–§B) and are out of scope here except where referenced.

---

## 1. Production build + ops docs (deferred M7 §8/§9)

- Multi-stage production `Dockerfile` (build workspace → slim runtime; `web/static` baked in). Only the
  dev `docker-compose.yml` exists today.
- `docs/deployment.md`: hosting, TLS termination, secrets (env/secret manager), email/OAuth/map/object
  storage, migrations as an explicit deploy step, health checks wired to the LB, rolling deploy +
  rollback (redeploy previous image; forward-only migrations → rollback = restore), **log retention**.
- `docs/backups.md`: daily logical backup (pg_dump) + WAL archiving, retention (default 30d), encryption
  at rest, restore procedure, RPO/RTO targets, and the **§98 interaction** — restore re-runs the
  `retention` command so deleted accounts are not resurrected.
- `docs/incident-response.md`: the §81 9-step flow (detection→classification→containment→impact→
  personal-data→escalation→notification→remediation→record); incident records written as audit events
  (`incident.*` action codes).
- Exercise restore at least once in a sandbox (documented in `docs/backups.md`).

## 2. Runtime plumbing of M7 config knobs (partial Ledger #18/#19)

- Thread `PhotoConfig` (max bytes / megapixels / thumbnail / quality) into the domain validation and
  the web `DefaultBodyLimit` and photo processor — currently the domain honours its compile-time
  constants and M7 only *exposed* the env knobs.
- Thread `ModerationConfig` (report description length, report rate limits) into `ModerationService` /
  its rate limiter.
- Confirm the recommendation weights + freshness thresholds actually drive all display categorization
  (home/detail view building still uses `DEFAULT_THRESHOLDS` in two spots).

## 3. i18n finish (partial M7 §7)

- Locale-aware **date** formatting (in addition to the money formatter shipped in M7).
- A final string audit over M2–M6 templates for any remaining hard-coded user-facing text.
- `hreflang` note: M7 points pt-BR/en at the *same* URL (locale is resolved by `Accept-Language` +
  `lang` cookie). Per-locale URLs (`/en/…` prefix + internal-link rewrite) are a routing change sized
  separately; **not** in scope for M8.

## 4. Real providers & production gating (reference only)

> Out of scope for M8 code decisions — see `PENDING_FOR_PRODUCTION.md` §A–§B.

## Explicitly NOT in M8 (unchanged from M7 / `PENDING_FOR_PRODUCTION.md`)

- Real geocoder/tiles/S3/OAuth/Redis rate limiter, `seed-mock`/`seed-admin`/`MEDIA_SIGNING_SECRET`
  gating, legal text review, optional features — all in `PENDING_FOR_PRODUCTION.md`.
- Browser E2E / axe-core / keyboard-only acceptance — removed from M8 scope.

---

## Suggested task breakdown

1. Production `Dockerfile`.
2. `docs/deployment.md`, `docs/backups.md`, `docs/incident-response.md`; sandbox restore exercise.
3. Runtime plumbing of `PhotoConfig`/`ModerationConfig`; display-categorization config threading.
4. Locale-aware date formatter + final string audit.
5. README updates; `PLAN.md` M8 status; final commit.
