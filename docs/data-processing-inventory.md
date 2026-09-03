# Data-processing inventory (§67/§68) + legal basis (§69)

> **Living document. Every legal basis is marked ⚠ for legal review** — the
> engineering plan must not invent legal bases (REQUIREMENTS §69). Last
> maintained by the M6 privacy milestone.

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

### Notes

- **None of the entries outbound-credit data to a third party beyond the
  recipients listed.** The app has no consent-based marketing, no advertising
  and no cross-site tracking (see §78).
- **§77 minimization** already holds from M1–M5 and is documented in
  `docs/provider-transfer-inventory.md`.
