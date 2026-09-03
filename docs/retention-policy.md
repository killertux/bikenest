# Retention policy (§75)

> **Bold = legal/product approval required.** The technical defaults are
> encoded in `bikenest_domain::RetentionPolicy` (Ledger #20 — made configurable
> in M7).

| Record | Period | Mechanism | Approval |
|---|---|---|---|
| password-reset token | 1 hour | `expires_at` + retention purge | technical default |
| email-verification token | 24 hours | `expires_at` + retention purge | technical default |
| session | 30 days idle / 90-day absolute cap | cookie Max-Age + `expires_at` + purge | technical default |
| "I parked here" | 90 days | `expires_at` + purge (and on deletion) | technical default |
| temporary privacy exports | 24 hours | `expires_at` + purge | technical default |
| temporary upload objects | 24 hours | orphan media sweep | technical default |
| **inactive accounts** | **config-gated, default off** | `INACTIVE_ACCOUNT_ANONYMIZE_AFTER_DAYS` | **⚠ legal/product** |
| **deleted (anonymized) account shells** | **config-gated, default off** | `DELETED_ACCOUNT_PURGE_AFTER_DAYS` | **⚠ legal/product** |
| **reviews / contributions / reports** | **long-term (retained anonymized)** | retained | **⚠ legal/product** |
| **audit events / security logs** | **long-term** | retained | **⚠ legal/product** |
| privacy requests | legal-review period | retained, `user_id` nulled | **⚠ legal/product** |

## Implementation surface

- The six technical-default purges are driven by `cargo run -p bikenest-web -- retention`.
- The two config-gated steps are **disabled by default** (`0`); they activate
  only when `INACTIVE_ACCOUNT_ANONYMIZE_AFTER_DAYS` / `DELETED_ACCOUNT_PURGE_AFTER_DAYS`
  are set in the environment — the periods are legal decisions and must be
  approved before use.
- The orphan media sweep removes object-storage files older than 24h that are no
  longer referenced by any `parking_photo`/`review_photo` row.
