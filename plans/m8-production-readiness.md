# M8 — Production & E2E (deferred from M7)

> **Status: planned (deferred from M7).** M7's *security & robustness hardening* and its *SEO/i18n
> core* are shipped (see `plans/m7-hardening.md`). This milestone is the remaining **E2E +
> accessibility**, the **production deployment/backups/incident-response docs + Dockerfile**, and the
> **runtime plumbing** of the config knobs that M7 exposed but did not yet thread through. The
> real-provider cutover and production gating live in `PENDING_FOR_PRODUCTION.md` (§A–§B) and are out
> of scope here except where referenced.

---

## 1. Playwright E2E + axe-core accessibility (deferred M7 §6)

- Add `web/e2e/` Playwright suite (`playwright.config.ts`, chromium) against `docker compose up`.
- `npm run e2e` runs the critical journeys (register→verify→search→add→review→favorite; moderation
  approve/resolve; report; account deletion/export/anonymization; CSP/Alpine smoke incl. mobile menu +
  lightbox under the strict CSP).
- `@axe-core/playwright` scan to WCAG 2.2 AA on home/search/details/add/login; keyboard-only pass on
  search→details→review (the non-map list already satisfies §63).
- **Why it matters most now:** the strict CSP + Alpine CSP build (M7) must be proven in a real browser;
  this is the only place that validates them end-to-end.

## 2. Production build + ops docs (deferred M7 §8/§9)

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
- Exercise restore at least once in a sandbox.

## 3. Runtime plumbing of M7 config knobs (partial Ledger #18/#19)

- Thread `PhotoConfig` (max bytes / megapixels / thumbnail / quality) into the domain validation and
  the web `DefaultBodyLimit` and photo processor — currently the domain honours its compile-time
  constants and M7 only *exposed* the env knobs.
- Thread `ModerationConfig` (report description length, report rate limits) into `ModerationService` /
  its rate limiter.
- Confirm the recommendation weights + freshness thresholds actually drive all display categorization
  (home/detail view building still uses `DEFAULT_THRESHOLDS` in two spots).

## 4. i18n finish (partial M7 §7)

- Locale-aware **date** formatting (in addition to the money formatter shipped in M7).
- A final string audit over M2–M6 templates for any remaining hard-coded user-facing text.
- Reconsider `hreflang` — M7 points pt-BR/en at the *same* URL (locale is resolved by
  `Accept-Language` + `lang` cookie). If per-locale URLs are desired, that is a routing change
  (`/en/…` prefix + internal-link rewrite) sized separately.

## 5. Real providers & production gating

> Out of scope for M8 code decisions — see `PENDING_FOR_PRODUCTION.md` §A–§B. M8 only needs the
> E2E suite to keep the fakes/dev impls testable while they are still wired.

## Explicitly NOT in M8 (unchanged from M7 / `PENDING_FOR_PRODUCTION.md`)

- Real geocoder/tiles/S3/OAuth/Redis rate limiter, `seed-mock`/`seed-admin`/`MEDIA_SIGNING_SECRET`
  gating, legal text review, optional features — all in `PENDING_FOR_PRODUCTION.md`.

---

## Suggested task breakdown

1. Playwright harness + `npm run e2e`; write the 5 critical journeys + CSP/Alpine smoke.
2. axe-core AA scan + fixes; keyboard-only pass.
3. Production `Dockerfile`.
4. `docs/deployment.md`, `docs/backups.md`, `docs/incident-response.md`; sandbox restore exercise.
5. Runtime plumbing of `PhotoConfig`/`ModerationConfig`; display-categorization config threading.
6. Locale-aware date formatter + final string audit; (optional) per-locale URLs.
7. README updates for E2E/deployment; `PLAN.md` M8 status; final commit.
