# Provider & international-transfer inventory

> **Decision 2026-09-03:** production hosting and **every** processor sit
> **outside Brazil** (EU and/or US). Every row is therefore an international
> transfer under LGPD art. 33 for Brazilian users, and — for EEA users — a
> Chapter V transfer whenever the provider is outside the EEA. The privacy
> policy (§6) says exactly this.
>
> **Mechanisms we rely on** (state them in each DPA):
> - **LGPD:** the ANPD standard contractual clauses (Resolução CD/ANPD nº
>   19/2024 — *Regulamento de Transferência Internacional de Dados*),
>   incorporated in the provider's DPA. If a provider does not offer them, the
>   fallback is art. 33 IX (transfer necessary to perform the contract with the
>   data subject) — weaker; prefer providers that sign the ANPD clauses.
> - **GDPR:** an adequacy decision (EU-hosted, or a US provider certified under
>   the EU-US Data Privacy Framework) or the EU standard contractual clauses in
>   the provider's DPA.
>
> **Status column** is the pre-launch checklist. Nothing below is done until the
> DPA is accepted in the provider account and the region is written down.

| Provider (chosen) | Purpose | Data transferred | Region | Role | GDPR mechanism | LGPD mechanism | Status |
|---|---|---|---|---|---|---|---|
| Hosting (app + PostgreSQL) — _name TBD_ | run app, store DB | full app + DB (all personal data) | ☐ EU / US — record it | processor | ☐ EU-hosted → none needed; US → DPF or EU SCC | ☐ ANPD SCC in DPA (else art. 33 IX) | ☐ DPA accepted ☐ region recorded ☐ backups same region |
| Object storage (S3-compatible: AWS S3 / Cloudflare R2 / Backblaze B2) — _TBD_ | photo binaries | derivative bytes under opaque keys (§77: no user metadata) | ☐ | processor | ☐ | ☐ | ☐ DPA ☐ region ☐ bucket private, presigned GET only |
| Email (Resend **or** SMTP relay) — _TBD_ | verification / reset mail | email address + token link | ☐ (Resend: US) | processor | ☐ | ☐ | ☐ DPA ☐ region |
| **Mapbox** (`GEOCODER=mapbox`) | address → coordinates | query string only (§77: no identity, cookie or client IP) | US | processor | ☐ Mapbox DPA (EU SCC / DPF) | ☐ ANPD SCC if offered, else art. 33 IX | ☐ DPA accepted ☐ token URL-restricted |
| Map tiles (Mapbox style **or** other via `MAP_STYLE_URL`) — _TBD_ | render basemap | tile requests **from the user's browser**: client IP + viewed area; no account identity | ☐ | processor (receives IP directly) | ☐ | ☐ | ☐ DPA ☐ attribution shown ☐ `CSP_TILE_HOSTS` set |
| Automated content screening (LLM/classifier) — _future_ | moderation assist | the photo/text being screened only; **never** account identity | ☐ | processor | ☐ | ☐ | not wired yet — add here + policy §5 already discloses it |
| Observability / error tracking — _none planned_ | logs/metrics | logs (headers never logged; PII minimized) | ☐ | processor | ☐ | ☐ | if added: DPA + region |
| Google (OAuth) | login | `sub`, email, `email_verified` | US | independent controller (their side) | n/a | n/a | **deferred — not in production**; add to policy §2/§4 when shipped |

## §77 minimization confirmations (already true from M1–M8)

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
  (§77). **Not sent:** account identity, cookies, client IP. Response
  `features[0]` (`center` as `[lon, lat]`) → `GeoHit`. Empty features → no match.

  **§83 documentation:** usage limit = Mapbox account plan/billing (free tier
  ~100k requests/month); ToS + attribution apply; caching is request-scoped (no
  client-side cache of results); terms-of-service and privacy are Mapbox's. A
  geocode error is rendered as a friendly "location service unavailable" page
  (not a 500).

## Pre-launch procedure (ops)

1. Pick the hosting, storage, email and tile providers; fill the _TBD_ cells.
2. In each provider console, accept the DPA; download/print it and note whether
   it includes **EU SCC / DPF** and the **ANPD standard clauses**. File them.
3. Record each region. Prefer EU regions for hosting/DB/storage: it removes the
   GDPR transfer question for EEA users entirely.
4. If any provider lacks ANPD clauses, note "art. 33 IX" in this table and flag
   it in `docs/legal-review.md` for counsel.
5. Set `CSP_TILE_HOSTS` / `CSP_GEOCODE_HOSTS` to exactly the chosen hosts.
