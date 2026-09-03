# Provider & international-transfer inventory (§76/§77)

> **Living document.** Per provider, what data crosses the boundary and the §77
> minimization guarantee. Every region is marked ⚠ for review.

| Provider | Purpose | Data transferred | Region | Role | Transfer mechanism | Retention | Deletion |
|---|---|---|---|---|---|---|---|
| Google (OAuth) | login | `sub`, email, `email_verified` (from Google) | ⚠ review | processor→controller | OAuth 2.0 | Google's own records | revoke/disconnect |
| Email provider (SMTP/Resend) | verification/reset mail | email, token link | ⚠ review | processor | TLS | provider-specific | provider DPA |
| Geocoding provider | address→coords | query string + coordinates (§77: no account identity) | ⚠ review | processor | HTTPS | request-scoped | — |
| Map/tile provider | render map | tile requests (§77: no authenticated identity) | ⚠ review | processor | HTTPS | request-scoped | — |
| Object-storage provider | photo binaries | derivative bytes under opaque keys (§77: no user metadata in keys) | ⚠ review | processor | HTTPS | until rejected/anonymized | `ObjectStorage::delete` |
| Hosting provider | run app | full app + DB | ⚠ review | processor | — | per SLA | standard |
| Observability / error tracking | logs/metrics | logs (no secrets; PII minimized) | ⚠ review | processor | — | per config | standard |

## §77 minimization confirmations (already true from M1–M5)

- **Map renderer** receives no authenticated identity — tiles are public.
- **Geocoder** receives only the query string.
- **Object store** receives only derivative bytes under opaque keys (no email or
  provider `sub` in keys).
- **Email provider** receives only the address + the verification/reset link.

These boundaries are asserted by M6 tests (the export payload never contains a
credential hash / token hash; the map/geocode/object-key calls carry no account
identity).
