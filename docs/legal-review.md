# Legal review record — privacy, terms, cookies

> Hand this file plus `policies/*.md` to counsel. It records **what product
> decided on 2026-09-03**, **why**, and **what still needs a lawyer**. The
> policy drafts were written by the engineering side to cover the information
> duties of LGPD art. 9 and GDPR arts. 13/14; they are not legal advice and
> have not been reviewed by counsel.

## 1. Decisions taken (product)

| Topic | Decision | Where it shows |
|---|---|---|
| Controller | A Brazilian company (name, CNPJ, address supplied via `POLICY_OPERATOR_*` at seed time) | policy §1, terms intro/§13 |
| Contact / encarregado | One monitored e-mail (`POLICY_CONTACT_EMAIL`) serves as the LGPD art. 41 channel and the GDPR contact. No separately named DPO | policy §1, §9, §13 |
| Jurisdictions | LGPD primary; GDPR language included for EEA/UK visitors | policy intro, §6, §9; terms §12 |
| Hosting / processors | **All outside Brazil (EU and/or US)** | policy §6; `docs/provider-transfer-inventory.md` |
| Transfer mechanism | ANPD standard contractual clauses via provider DPAs; fallback LGPD art. 33 IX. GDPR: adequacy / DPF / EU SCC | policy §6 |
| Minimum age | **18+** — avoids LGPD art. 14 and GDPR art. 8 entirely | policy §10, terms §2, sign-up form |
| Legal bases | contract for account + contributions; legitimate interest for security/moderation/audit; legal obligation for Marco Civil access logs and the rights-request record. **No consent-based processing** | policy §3; `docs/data-processing-inventory.md` |
| Retention | inactive accounts: **not** auto-anonymized; deleted shells purged 30 days; audit + privacy requests 5 years; access logs 6 months; the rest = §75 technical defaults | policy §7; `docs/retention-policy.md` |
| Cookies | 3 first-party essential/functional cookies, no banner, no consent | cookies policy |
| UGC responsibility | user warrants authorship, no faces/plates/private interiors, no obscene/illegal content; indemnity; perpetual licence; anonymous display | terms §3 |
| Moderation | photos held until approved; human moderators **plus automated tools (LLM classifiers) disclosed now** even though the automated part ships later; flagged content always goes to a human; review on request | policy §5, terms §4 |
| Takedown channel | report button + contact e-mail; notice must carry URL + reason + contact | terms §4 |
| Liability | "as is"; explicit no-guarantee of bike safety/theft; limits "to the extent permitted by law" (CDC preserved) | terms §7–§8 |
| Governing law / forum | Brazil; company's seat, with the consumer's domicile preserved (CDC art. 101 I) and mandatory foreign consumer protections acknowledged | terms §12 |

## 2. Legitimate-interest balancing note (LGPD art. 10 / GDPR art. 6(1)(f))

- **Purpose:** keep a public UGC map safe — sessions, rate limits, audit trail,
  moderation of photos/texts, handling reports.
- **Necessity:** no less intrusive way to prevent abuse of a public upload
  surface; IP/user-agent are used only transiently for limits (not stored by
  the app); audit rows record *who did what* for accountability.
- **Impact:** low — no profiling, no marketing, nothing sold; uploader identity
  is never public; users can object/restrict via the privacy hub.
- **Safeguards:** §77 minimization to providers, EXIF stripping, access
  control, retention limits, anonymization on deletion.

## 3. Points for counsel to confirm or fix

1. **Wording review** of all six files (`policies/*.{pt-BR,en}.md`), especially
   the terms' licence (§3.3), indemnity (§3.4) and liability limits (§8) against
   the CDC, and whether the anonymous-display clause is compatible with moral
   rights (Lei 9.610 art. 24) — it is framed as the author's choice, not a waiver.
2. **Encarregado:** we rely on a contact channel rather than a named DPO. If the
   company is not an *agente de tratamento de pequeno porte* (Res. CD/ANPD
   2/2022), a named encarregado may be required — add the name to policy §1.
3. **International transfers:** confirm the ANPD-SCC-via-DPA approach and the
   art. 33 IX fallback; tell us if any chosen provider's DPA is insufficient.
4. **GDPR applicability / art. 27 representative:** we target Brazil; EEA use is
   incidental. We have **not** appointed an EU representative (art. 27(2)(a)
   exemption assumed). Confirm.
5. **Marco Civil art. 15:** we treat the company as an application provider
   "com fins econômicos" and keep access logs 6 months at the proxy. Confirm
   applicability and that proxy logs satisfy the "controlled environment" duty.
6. **Marco Civil art. 19 after the STF decision (June 2025):** confirm the
   notice-and-takedown duties that apply to a small UGC platform and whether the
   terms §4 channel and our moderation SLA are adequate.
7. **Automated moderation disclosure:** confirm the art. 20 LGPD / art. 22 GDPR
   framing (human in the loop, review on request) is enough once the LLM
   classifier ships; we will add the model provider to the transfer inventory.
8. **Retention numbers:** 5 years for audit + rights-request records, 30-day
   shell purge, indefinite anonymized contributions — confirm.
9. **Age 18+:** confirm no additional verification duty beyond the declaration.

## 4. Operational to-dos before launch (not legal, but promised by the text)

- Set `POLICY_OPERATOR_NAME/CNPJ/ADDRESS`, `POLICY_CONTACT_EMAIL`,
  `POLICY_VERSION`, `POLICY_EFFECTIVE_AT`; run `seed-policies`.
- Monitor the contact inbox: rights requests (15 days LGPD / 1 month GDPR) and
  takedown notices.
- Accept DPAs + record regions per `docs/provider-transfer-inventory.md`.
- Proxy access-log retention = 6 months; diagnostic logs ≈ 30 days.
- `DELETED_ACCOUNT_PURGE_AFTER_DAYS=30`; keep `INACTIVE_ACCOUNT_ANONYMIZE_AFTER_DAYS=0`.
- Hide/disable the fake Google login in production.
- When policy text changes: bump `POLICY_VERSION`, reseed, and notify users
  before `POLICY_EFFECTIVE_AT` (the policies promise it).
