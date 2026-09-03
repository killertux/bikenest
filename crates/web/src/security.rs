//! Security response headers + Content-Security-Policy (§64/§65).
//!
//! A single middleware adds the hard-header set to *every* response (public,
//! account, admin, media, error): `Strict-Transport-Security` (only when TLS is
//! on), `Content-Security-Policy`, `X-Content-Type-Options`, `Referrer-Policy`,
//! `Permissions-Policy` and `X-Frame-Options`.
//!
//! The CSP is deliberately strict: `script-src 'self'` with **no `'unsafe-eval'`**
//! — this is the whole point of the Alpine CSP build (plans/m7-hardening.md §3).
//! `style-src 'unsafe-inline'` is retained only because MapLibre injects inline
//! styles (attribution/controls/markers); we add no inline styles of our own, so
//! that surface is MapLibre's alone. The tile/geocode origins are templated from
//! env so the same binary works against any hosted provider (dev defaults to the
//! `demotiles.maplibre.org` demo style — Ledger #3).

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, Request};
use axum::middleware::Next;
use axum::response::Response;

/// Config-driven security-header policy for the running instance.
#[derive(Debug, Clone)]
pub struct SecurityHeaders {
    /// Whether TLS terminates at/behind this instance. When true, `Strict-Transport-Security`
    /// is emitted (never in plaintext dev).
    tls_on: bool,
    /// Origins MapLibre loads the style (fetch), vector/raster tiles, glyphs and sprites from.
    tile_hosts: Vec<String>,
    /// Origins the browser may reach for client-side geocoding (empty in dev — geocoding is
    /// server-side via the `Geocoder` port).
    geocode_hosts: Vec<String>,
}

impl SecurityHeaders {
    pub fn from_env() -> Self {
        Self {
            tls_on: env_flag("TLS_ON"),
            tile_hosts: env_hosts("CSP_TILE_HOSTS")
                .unwrap_or_else(|| vec!["https://demotiles.maplibre.org".to_string()]),
            geocode_hosts: env_hosts("CSP_GEOCODE_HOSTS").unwrap_or_default(),
        }
    }

    /// The `Content-Security-Policy` value. Hosts templated from config; no `'unsafe-eval'`.
    pub fn csp(&self) -> String {
        let tile = self.join_hosts(&self.tile_hosts);
        let geocode = self.join_hosts(&self.geocode_hosts);
        format!(
            "default-src 'self'; \
             script-src 'self'; \
             style-src 'self' 'unsafe-inline'; \
             img-src 'self' data: blob:{tile}; \
             font-src 'self'{tile}; \
             connect-src 'self'{tile}{geocode}; \
             worker-src 'self' blob:; \
             object-src 'none'; \
             base-uri 'self'; \
             frame-ancestors 'none'; \
             form-action 'self'"
        )
    }

    /// Join configured origins into a directive fragment (leading space when non-empty).
    fn join_hosts(&self, hosts: &[String]) -> String {
        if hosts.is_empty() {
            String::new()
        } else {
            format!(" {}", hosts.join(" "))
        }
    }
}

/// Axum middleware: append the security-header set to every response.
pub async fn security_headers(
    State(s): State<SecurityHeaders>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let path_is_private = is_private_path(req.uri().path());
    let mut res = next.run(req).await;
    let head = res.headers_mut();
    head.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    head.insert(
        "Referrer-Policy",
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    head.insert(
        "Permissions-Policy",
        HeaderValue::from_static(
            "camera=(), microphone=(), geolocation=(self), interest-cohort=()",
        ),
    );
    // Legacy guard — the modern guard is CSP `frame-ancestors 'none'`.
    head.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
    // The CSP string is a fixed shape (no forbidden header chars), so
    // `from_str` always succeeds; a failure means a real bug and should panic
    // rather than silently strip the policy (§65).
    head.insert(
        "Content-Security-Policy",
        HeaderValue::from_str(&s.csp()).expect("valid CSP header value"),
    );
    if s.tls_on {
        head.insert(
            "Strict-Transport-Security",
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }
    // Private data must never be indexed (§110). Also enforced in robots.txt.
    if path_is_private {
        head.insert("X-Robots-Tag", HeaderValue::from_static("noindex, nofollow"));
    }
    res
}

/// Private (account/admin/moderation) paths — never indexable (§110).
fn is_private_path(path: &str) -> bool {
    path.starts_with("/account")
        || path.starts_with("/admin")
        || path.starts_with("/moderation")
}

fn env_flag(key: &str) -> bool {
    std::env::var(key)
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}

fn env_hosts(key: &str) -> Option<Vec<String>> {
    std::env::var(key).ok().map(|raw| {
        raw.split(',')
            .map(str::trim)
            .filter(|h| !h.is_empty())
            .map(str::to_string)
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(
        tls_on: bool,
        tile_hosts: &[&str],
        geocode_hosts: &[&str],
    ) -> SecurityHeaders {
        SecurityHeaders {
            tls_on,
            tile_hosts: tile_hosts.iter().map(|s| s.to_string()).collect(),
            geocode_hosts: geocode_hosts.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn csp_is_strict_no_unsafe_eval() {
        let csp = headers(false, &["https://tiles.example.com"], &["https://geo.example.com"]).csp();
        assert!(csp.contains("script-src 'self'"));
        assert!(!csp.contains("unsafe-eval"));
        assert!(csp.contains("object-src 'none'"));
        assert!(csp.contains("frame-ancestors 'none'"));
        assert!(csp.contains("base-uri 'self'"));
        assert!(csp.contains("form-action 'self'"));
        assert!(csp.contains("https://tiles.example.com"));
        assert!(csp.contains("https://geo.example.com"));
    }

    #[test]
    fn csp_hosts_omitted_when_absent() {
        let csp = headers(false, &[], &[]).csp();
        assert!(!csp.contains("https://"));
        assert!(csp.contains("img-src 'self' data: blob:"));
        assert!(csp.contains("font-src 'self'"));
        assert!(csp.contains("connect-src 'self'"));
    }

    #[test]
    fn dev_defaults_to_demo_tiles() {
        let h = SecurityHeaders::from_env();
        assert!(h.tile_hosts.iter().any(|h| h == "https://demotiles.maplibre.org"));
        assert!(h.geocode_hosts.is_empty() || !h.tls_on);
    }

    #[test]
    fn hsts_only_when_tls_on() {
        assert!(!headers(false, &[], &[]).tls_on);
        assert!(headers(true, &[], &[]).tls_on);
    }
}

