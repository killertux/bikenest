//! htmx request discipline (htmx 4.0.0, vendored at `web/static/vendor/htmx.js`).
//!
//! htmx 4 puts `HX-Request: true` on **every** request it issues, not only on
//! the fragment swaps a handler wants to answer with a partial:
//!
//! * `#createCoreHeaders` always sends `HX-Request`, `HX-Source`,
//!   `HX-Current-URL` and `Accept: text/html`, and adds `HX-Boosted: true`
//!   when the source element is boosted;
//! * just before the fetch, htmx sets
//!   `HX-Request-Type = (ctx.target === document.body || ctx.select) ? "full" : "partial"`;
//! * a back/forward navigation replays the page through `#restoreHistory`,
//!   which targets `document.body` and adds `HX-History-Restore-Request: true`.
//!
//! So "the client sent `HX-Request`" is **not** "the client wants a fragment".
//! A boosted navigation and a history restore both swap into `<body>`; answering
//! either with a partial makes that partial the entire document. Every endpoint
//! whose success response is a fragment therefore asks [`is_fragment_request`]
//! first and falls back to a plain redirect (which also gives the no-JS path a
//! correct POST/redirect/GET flow).

use axum::http::{HeaderMap, HeaderValue, Method, Uri, header};
use axum::response::{IntoResponse, Redirect, Response};

/// Sent by htmx on every request it issues (`#createCoreHeaders`).
pub const HX_REQUEST: &str = "hx-request";
/// `"full"` when the resolved target is `document.body` or a `select` is in
/// play, `"partial"` otherwise.
pub const HX_REQUEST_TYPE: &str = "hx-request-type";
/// Present only on `hx-boost`ed links/forms (`#createCoreHeaders`).
pub const HX_BOOSTED: &str = "hx-boosted";
/// Present on the GET htmx replays for a back/forward navigation.
pub const HX_HISTORY_RESTORE: &str = "hx-history-restore-request";
/// The page the request was issued from (`location.href`).
pub const HX_CURRENT_URL: &str = "hx-current-url";
/// Response header htmx turns into a client-side `location.href` assignment,
/// honoured for any status (`#handleHeadersAndMaybeReturnEarly`).
pub const HX_REDIRECT: &str = "hx-redirect";

/// The response varies by every header that selects between the fragment and
/// the full document, plus the two that select the locale/session rendering.
pub const VARY_FRAGMENT: &str =
    "HX-Request, HX-Request-Type, HX-Boosted, Accept-Language, Cookie";

/// The subset that every HTML response varies by (added by the security-header
/// middleware; fragment endpoints add [`VARY_FRAGMENT`] instead).
pub const VARY_HTML: &str = "Accept-Language, Cookie";

/// True only for a request that wants a *fragment* swapped into a real target:
/// `HX-Request` is present, it is not boosted, it is not a history restore, and
/// htmx did not resolve the target to `document.body` (`HX-Request-Type: full`).
pub fn is_fragment_request(headers: &HeaderMap) -> bool {
    if !headers.contains_key(HX_REQUEST) {
        return false;
    }
    if headers.contains_key(HX_BOOSTED) || headers.contains_key(HX_HISTORY_RESTORE) {
        return false;
    }
    !headers
        .get(HX_REQUEST_TYPE)
        .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"full"))
}

/// The fragment for a real fragment request; otherwise a 303 to the page that
/// shows the same state (post/redirect/get for the no-JS and boosted paths).
pub fn fragment_or_redirect(
    headers: &HeaderMap,
    fragment: Response,
    redirect_to: &str,
) -> Response {
    if is_fragment_request(headers) {
        vary_fragment(fragment)
    } else {
        Redirect::to(redirect_to).into_response()
    }
}

/// Append (never clobber) the `Vary` names a fragment endpoint's response
/// depends on. Appending keeps any `Vary` a lower layer already set.
pub fn vary_fragment(mut resp: Response) -> Response {
    resp.headers_mut()
        .append(header::VARY, HeaderValue::from_static(VARY_FRAGMENT));
    resp
}

/// Reduce a caller-supplied string to a safe *local* path, or `None`.
///
/// A local path must start with a single `/` and contain no backslash (some
/// browsers normalise `\` to `/`, so `/\evil.com` would otherwise leave the
/// origin) and no control characters. Everything else — a scheme, a
/// protocol-relative `//host`, a bare word — is rejected rather than normalised.
pub fn safe_local_path(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'/') {
        return None;
    }
    if matches!(bytes.get(1), Some(b'/') | Some(b'\\')) {
        return None;
    }
    if bytes
        .iter()
        .any(|&b| b == b'\\' || b < 0x20 || b == 0x7F)
    {
        return None;
    }
    Some(s)
}

/// Strip an absolute URL down to its path + query, so a `Referer` or
/// `HX-Current-URL` can be fed to [`safe_local_path`].
fn local_part(raw: &str) -> Option<&str> {
    let rest = match raw.find("://") {
        Some(i) => {
            let after = &raw[i + 3..];
            match after.find('/') {
                Some(j) => &after[j..],
                None => "/",
            }
        }
        None => raw,
    };
    safe_local_path(rest)
}

/// Where a login should send the user back to.
///
/// * A safe method (`GET`/`HEAD`) is its own page: use the request's path+query.
/// * A state-changing method's path is an *action*, not a page
///   (`POST /parking/7/favorite` is not GET-able), so we use the page it was
///   issued from: `HX-Current-URL` (htmx sends it on every request), else
///   `Referer`, else the request path minus its final verb segment
///   (`/parking/7/favorite` → `/parking/7`), else `/`.
pub fn login_next(method: &Method, uri: &Uri, headers: &HeaderMap) -> String {
    if method == Method::GET || method == Method::HEAD {
        let path_and_query = uri
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or_else(|| uri.path());
        if let Some(p) = safe_local_path(path_and_query) {
            return p.to_string();
        }
        return "/".to_string();
    }
    for name in [HX_CURRENT_URL, "referer"] {
        if let Some(raw) = headers.get(name).and_then(|v| v.to_str().ok())
            && let Some(local) = local_part(raw)
        {
            return local.to_string();
        }
    }
    let path = uri.path();
    match path.rfind('/') {
        // `/parking/7/favorite` → `/parking/7`; `/reports` → `/`.
        Some(0) | None => "/".to_string(),
        Some(i) => safe_local_path(&path[..i])
            .unwrap_or("/")
            .to_string(),
    }
}

/// Percent-encode a value for a query string (`next=…`).
pub fn encode_query_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hx(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn plain_request_is_never_a_fragment_request() {
        assert!(!is_fragment_request(&HeaderMap::new()));
    }

    #[test]
    fn bare_hx_request_is_a_fragment_request() {
        assert!(is_fragment_request(&hx(&[("hx-request", "true")])));
        assert!(is_fragment_request(&hx(&[
            ("hx-request", "true"),
            ("hx-request-type", "partial"),
        ])));
    }

    #[test]
    fn boosted_history_and_full_target_requests_want_a_document() {
        // A boosted navigation swaps `document.body`.
        assert!(!is_fragment_request(&hx(&[
            ("hx-request", "true"),
            ("hx-boosted", "true"),
            ("hx-request-type", "full"),
        ])));
        // A back/forward replay targets `document.body` too.
        assert!(!is_fragment_request(&hx(&[
            ("hx-request", "true"),
            ("hx-history-restore-request", "true"),
        ])));
        // `hx-target="body"` (or an `hx-select`) on an ordinary request.
        assert!(!is_fragment_request(&hx(&[
            ("hx-request", "true"),
            ("hx-request-type", "FULL"),
        ])));
    }

    #[test]
    fn safe_local_path_accepts_only_single_slash_local_paths() {
        assert_eq!(safe_local_path("/"), Some("/"));
        assert_eq!(safe_local_path("/ok/path?x=1"), Some("/ok/path?x=1"));
        assert_eq!(safe_local_path("/parking/7"), Some("/parking/7"));
    }

    #[test]
    fn safe_local_path_rejects_open_redirects() {
        assert_eq!(safe_local_path("//evil.com"), None);
        assert_eq!(safe_local_path(r"/\evil.com"), None);
        assert_eq!(safe_local_path(r"/ok\evil.com"), None);
        assert_eq!(safe_local_path("javascript:alert(1)"), None);
        assert_eq!(safe_local_path("https://evil.com"), None);
        assert_eq!(safe_local_path("evil.com"), None);
        assert_eq!(safe_local_path(""), None);
        assert_eq!(safe_local_path("/ok\nSet-Cookie: x=1"), None);
    }

    #[test]
    fn login_next_uses_the_path_for_safe_methods() {
        let uri: Uri = "/account/favorites?page=2".parse().unwrap();
        assert_eq!(
            login_next(&Method::GET, &uri, &HeaderMap::new()),
            "/account/favorites?page=2"
        );
    }

    #[test]
    fn login_next_for_a_post_prefers_the_page_it_came_from() {
        let uri: Uri = "/parking/7/favorite".parse().unwrap();
        let headers = hx(&[("hx-current-url", "https://bikenest.test/parking/7?x=1")]);
        assert_eq!(login_next(&Method::POST, &uri, &headers), "/parking/7?x=1");
    }

    #[test]
    fn login_next_for_a_post_falls_back_to_the_parent_path() {
        let uri: Uri = "/parking/7/favorite".parse().unwrap();
        assert_eq!(
            login_next(&Method::POST, &uri, &HeaderMap::new()),
            "/parking/7"
        );
        let root: Uri = "/reports".parse().unwrap();
        assert_eq!(login_next(&Method::POST, &root, &HeaderMap::new()), "/");
    }

    #[test]
    fn login_next_ignores_a_cross_origin_referer() {
        let uri: Uri = "/parking/7/favorite".parse().unwrap();
        // A foreign origin contributes only its path, so the result can never
        // leave this origin.
        let headers = hx(&[("referer", "https://evil.com")]);
        assert_eq!(login_next(&Method::POST, &uri, &headers), "/");
        let headers = hx(&[("referer", "https://evil.com/parking/7")]);
        assert_eq!(login_next(&Method::POST, &uri, &headers), "/parking/7");
    }

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(encode_query_value("/parking/7"), "/parking/7");
        assert_eq!(encode_query_value("/a b"), "/a%20b");
        assert_eq!(encode_query_value("//evil"), "//evil");
    }
}
