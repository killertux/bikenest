# Cookie Policy

> **Placeholder legal text — requires legal review before production (§71).**
> Seeded into `policy_version` by `cargo run -p bikenest-web -- seed-policies`.

## Cookies we set

We use a small number of **necessary** cookies. None of them are optional,
advertising or analytics cookies.

| Cookie | Purpose | Duration | Type |
|---|---|---|---|
| `session_id` | Keeps you signed in (HttpOnly, Secure, SameSite=Lax) | 30 days | Necessary |
| `csrf` | Protects forms against cross-site request forgery (HttpOnly, SameSite=Lax) | 1 hour | Necessary — security |
| `lang` | Remembers your language preference | 1 year | Necessary — functional |

## Third-party cookies

We do not allow third parties to set cookies, and we do not embed advertising or
analytics trackers.
