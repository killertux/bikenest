# Retention policy

> Periods marked **decided 2026-09-03** were set by product (see
> `docs/legal-review.md`); the technical defaults are encoded in
> `bikenest_domain::RetentionPolicy` (Ledger #20, env-configurable). The privacy
> policy (`policies/privacy.*.md` §7) states the same table — keep in sync.

| Record | Period | Mechanism | Status |
|---|---|---|---|
| password-reset token | 1 hour | `expires_at` + retention purge | technical default |
| email-verification token | 24 hours | `expires_at` + retention purge | technical default |
| session | 30 days idle / 90-day absolute cap | cookie Max-Age + `expires_at` + purge | technical default |
| "I parked here" | 90 days | `expires_at` + purge (and on deletion) | technical default |
| temporary privacy exports | 24 hours | `expires_at` + purge | technical default |
| temporary upload objects / rejected photos | 24 hours | orphan media sweep | technical default |
| **inactive accounts** | **not anonymized automatically** (`INACTIVE_ACCOUNT_ANONYMIZE_AFTER_DAYS=0`) | config-gated step stays off | **decided 2026-09-03** — no reminder e-mail exists; the policy promises advance notice before any inactivity deletion. Revisit if a reminder flow ships |
| **deleted (anonymized) account shells** | **30 days** (`DELETED_ACCOUNT_PURGE_AFTER_DAYS=30`) | `retention` command hard-deletes `users` rows with `account_state='DELETED' AND deleted_at < now()-30d` | **decided 2026-09-03** — the shell holds no personal data (email is `deleted+<id>@bikenest.invalid`), the purge is housekeeping |
| **reviews / contributions / photos** | retained as the community dataset, **anonymized** on account deletion | anonymize-in-place (§74) | **decided 2026-09-03** — anonymized rows are no longer personal data |
| **reports / moderation records** | retained for service safety; reporter anonymized on deletion | anonymize-in-place | **decided 2026-09-03** |
| **access logs (date/time + IP)** | **6 months** | reverse-proxy / LB log retention (ops — `docs/deployment.md` §7) | **legal obligation** — Marco Civil da Internet art. 15 |
| **audit events** | **5 years** | manual/ops purge (no automatic step yet — see below) | **decided 2026-09-03**; aligned with the 5-year limitation period for consumer claims (CDC art. 27) |
| **privacy requests** | **5 years**, `user_id` nulled on deletion | manual/ops purge | **decided 2026-09-03** |
| diagnostic logs | ~30 days | log driver | ops default |

## Implementation surface

- The six technical-default purges plus the 30-day shell purge are driven by
  `cargo run -p bikenest-web -- retention` (schedule it daily; see
  `docs/deployment.md`).
- `INACTIVE_ACCOUNT_ANONYMIZE_AFTER_DAYS` **must stay 0** unless the policy text
  is changed first (it promises notice).
- **Not yet automated:** the 5-year purge of `audit_events` and
  `privacy_request` rows. Nothing reaches that age before 2031; add a
  `retention` step (config-gated, `AUDIT_RETENTION_DAYS`) before then, or run a
  yearly manual `DELETE ... WHERE created_at < now() - interval '5 years'`.
- **Not in the app:** the 6-month access-log retention lives at the proxy /
  hosting layer. Verify it is configured before launch.
