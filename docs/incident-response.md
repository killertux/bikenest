# Incident response (§81)

> **What this covers:** how to investigate and react to a **security incident
> involving personal data**. The flow is §81's nine steps. Alongside the audit
> log (action codes) and the request/export/anonymization commands (§67/§98), the
> detection and record hooks are already present; this document is the runbook.
>
> **Every incident gets an incident record.** Records are written as `audit_events`
> rows with `incident.*` action codes, so they share the protection/permissioning
> of the audit log and survive on a separate retention from diagnostic logs.

---

## The flow

```
Detection → Classification → Containment → Impact assessment
→ Personal-data assessment → Internal escalation → Regulatory/user notification
→ Remediation → Incident record
```

## 1. Detection

- **Structured logs** (`APP_ENV=production`): request-level `TraceLayer` lines
  (method/path/status/latency; headers never logged — no cookie/token/PII leak) and
  PII-free `info`/`warn` events at key boundaries (login failure, report, photo
  upload/moderation, privacy request/export).
- **Audit events** (`audit_events` table): every security-relevant action is an
  `action` code with actor, target, and result — e.g. `auth.login`,`photo.*`,
  `report.*`, `privacy.*`, `retention.*`, `moderation.*`. This is the investigation
  basis.
- **Alerts** on: a burst of login failures; `*_rate_limited` events; a privacy
  export/delete spike; moderation or audit-write errors; repeated 5xx from the
  email/OAuth/object-store providers. Wire these to your alerting channel.

## 2. Classification

Determine severity (Low / Medium / High / Critical) and whether it is a **personal
data** incident (in scope of §81/§82) or purely operational. Consult
`docs/data-processing-inventory.md` for what personal data is stored where, and the
existing `docs/provider-transfer-inventory.md` for third-party transfer.

## 3. Containment

Stop the bleed before investigating fully:

- Rotate the involved secret (`MEDIA_SIGNING_SECRET`, DB password, email/OAuth
  keys, `ADMIN_PASSWORD`).
- Revoke sessions / tokens if credentials were exposed (the export/anonymization
  commands or a manual session purge).
- If a photo/object or a moderation decision is the subject, set it
  `Hidden`/reject it (M5 has hide/restore).
- If the DB was affected, isolate the instance (do **not** let it serve traffic)
  and stage for restore — see `docs/backups.md` §6.

## 4. Impact assessment

What actually changed? Use the audit log to bound the blast radius: which accounts,
which targets, what actions, from when. Compare against the data inventory.

## 5. Personal-data assessment

Identify every category of personal data touched (identity, contact, location
history, reviews, photos, browser location, authentication, audit), where it was
processed, and whether it crossed a third-party boundary (geocoder, map/tile
provider, email/ESP, object storage, OAuth). Re-read `docs/data-processing-inventory.md`.

## 6. Internal escalation

Escalate to the owner/DPO-equivalent and, if a data breach is confirmed, to senior
management and security/legal. Record the escalation time in the incident record.

## 7. Regulatory / user notification

If legally required (e.g. a reportable breach under your jurisdiction), notify the
supervisory authority and the affected data subjects within the statutory window.
Confirm the notification basis with legal before sending. If user notification is
appropriate, use the email provider (envelope `EMAIL_FROM`).

## 8. Remediation

Fix the root cause (patch, secret rotation, config change, new acceptance test),
then verify. The M7/M8 hardening is the baseline: strict CSP, no `unsafe-eval`,
no header/PII logging, env-tunable limits, `X-Robots-Tag` noindex on private
paths. Extend it where the incident revealed a gap.

## 9. Incident record

Close with a record that is **itself protected** (§81: "Incident records MUST
themselves be protected"). Write it as audit rows:

- `incident.opened` — summary, classification, detection source.
- `incident.contained` — what was contained, secrets rotated.
- `incident.assessed` — impact + personal-data scope.
- `incident.escalated` — who/when.
- `incident.notified` — regulatory/user notification outcome.
- `incident.resolved` — remediation + verification + a post-incident review entry.

Because these are `audit_events` rows, they inherit the audit table's access
controls and are queryable. Keep them out of the generic diagnostic log stream and
retain them per your audit retention policy (`docs/retention-policy.md`).

## Log & record retention

Audit events and incident records are **long-term** (retained, see
`docs/retention-policy.md`; **bold** = legal/product approval for the window).
Diagnostic logs are retained 30 days by default. Separate the two — a long-lived
incident record must not be lost with the diagnostic log rotation.
