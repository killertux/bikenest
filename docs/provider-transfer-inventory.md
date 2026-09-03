# Provider & international-transfer inventory (§76/§77)

> **Living document.** Per provider, what data crosses the boundary and the §77
> minimization guarantee. Every region is marked ⚠ for review.

| Provider | Purpose | Data transferred | Region | Role | Transfer mechanism | Retention | Deletion |
|---|---|---|---|---|---|---|---|
| Google (OAuth) | login | `sub`, email, `email_verified` (from Google) | ⚠ review | processor→controller | OAuth 2.0 | Google's own records | revoke/disconnect |
| Email provider (SMTP/Resend) | verification/reset mail | email, token link | ⚠ review | processor | TLS | provider-specific | provider DPA |
| Geocoding provider (**Mapbox**, via `GEOCODER=mapbox`) | address→coords | query string + resolved coordinates (§77: no account identity) | ⚠ review | processor | HTTPS (server-side) | request-scoped | — |
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

## Geocoder (§83) — selectable backend

`crates/infrastructure/src/geocoding.rs` (`GEOCODER`, default `fake`):

- **fake** — deterministic dev geocoder (no external request).
- **mapbox** — `MapboxGeocoder` → `GET api.mapbox.com/geocoding/v5/mapbox.places/{query}.json`
  with `limit=1`. **Sent to Mapbox:** only the percent-encoded query string
  (`$77`). **Not sent:** account identity, cookies, client IP. Response
  `features[0]` (`center` as `[lon, lat]`) → `GeoHit`. Empty features → no match.

  **§83 documentation:** usage limit = Mapbox account plan/billing (free tier
  ~100k requests/month); ToS + attribution apply; caching is request-scoped (no
  client-side cache of results); terms-of-service and privacy are Mapbox's. A
  geocode error is rendered as a friendly "location service unavailable" page
  (not a 500). **Legal review (PENDING §C):** provider contract / DPA and
  international-transfer assessment for Mapbox (US-based).

