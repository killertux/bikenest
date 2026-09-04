//! Authentication middleware + extractors (M2). Resolves the session cookie
//! into a principal, enforces CSRF on state-changing authenticated requests,
//! and hands handlers an [`Auth`] principal.

use axum::body::{Body, to_bytes};
use axum::extract::{FromRequestParts, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use bikenest_application::{AuthenticatedUser, TokenGenerator};
use bikenest_domain::{CsrfToken, SessionId};

use crate::http::AppState;

/// Name of the session cookie.
pub const SESSION_COOKIE: &str = "session_id";
/// Name of the synchronizer-token header / form field.
pub const CSRF_HEADER: &str = "x-csrf-token";
/// Name of the anonymous double-submit CSRF cookie (§108 — protects pre-session
/// requests like login/register/reset, which have no session row yet).
pub const ANON_CSRF_COOKIE: &str = "csrf";

/// The resolved principal for the current request. Always present on requests
/// flowing through the auth middleware (anonymous when no session).
#[derive(Debug, Clone, Default)]
pub struct Auth {
    pub user: Option<AuthenticatedUser>,
    pub csrf: Option<CsrfToken>,
    /// The raw session id (for revoking the current session).
    pub session: Option<SessionId>,
}

impl Auth {
    pub fn authenticated(&self) -> bool {
        self.user.is_some()
    }

    /// Returns the authenticated principal, or a redirect to `/login` when
    /// anonymous (302, with a `next` back to the requested path).
    #[allow(clippy::result_large_err)]
    pub fn require_user(&self) -> Result<&AuthenticatedUser, Response> {
        self.user
            .as_ref()
            .ok_or_else(|| Redirect::to("/login").into_response())
    }

    /// Returns the authenticated principal iff it has `role`, else 403.
    #[allow(clippy::result_large_err)]
    pub fn require_role(
        &self,
        role: bikenest_domain::Role,
    ) -> Result<&AuthenticatedUser, Response> {
        let user = self.require_user()?;
        if user.has_role(role) {
            Ok(user)
        } else {
            Err((StatusCode::FORBIDDEN, "Forbidden").into_response())
        }
    }

    /// Returns the authenticated principal iff they are a moderator **or** an
    /// admin (the M4 moderation queue grants both — plan §7).
    #[allow(clippy::result_large_err)]
    pub fn require_moderator(&self) -> Result<&AuthenticatedUser, Response> {
        let user = self.require_user()?;
        if user.has_role(bikenest_domain::Role::Moderator)
            || user.has_role(bikenest_domain::Role::Admin)
        {
            Ok(user)
        } else {
            Err((StatusCode::FORBIDDEN, "Forbidden").into_response())
        }
    }

    /// Returns the authenticated principal iff their email is verified (the
    /// §16 contribution gate), else 403 with the verification notice. Applies
    /// to add/edit/proposal/review/verify; favorites use [`Self::require_user`].
    #[allow(clippy::result_large_err)]
    pub fn require_verified(&self) -> Result<&AuthenticatedUser, Response> {
        let user = self.require_user()?;
        if user.is_verified {
            Ok(user)
        } else {
            Err((StatusCode::FORBIDDEN, "Verify your email to contribute").into_response())
        }
    }

    /// The session's CSRF token for rendering into a form / meta tag.
    pub fn csrf_value(&self) -> String {
        self.csrf
            .as_ref()
            .map(|c| c.to_base64url())
            .unwrap_or_default()
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Auth {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        Ok(parts.extensions.get::<Auth>().cloned().unwrap_or_default())
    }
}

// ---------------------------------------------------------------------------
// Cookie helpers
// ---------------------------------------------------------------------------

/// Read the raw session id from the `session_id` cookie, if present.
fn session_id_from_headers(headers: &HeaderMap) -> Option<String> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    cookie
        .split(';')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| k.trim() == SESSION_COOKIE)
        .map(|(_, v)| v.trim().to_string())
}

/// `Set-Cookie` header that persists the raw session id (HttpOnly, Secure,
/// SameSite=Lax, Path=/; §18).
pub fn set_session_cookie(id: &SessionId) -> String {
    format!(
        "{SESSION_COOKIE}={}; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=2592000",
        id.to_hex()
    )
}

/// `Set-Cookie` header that clears the session cookie.
pub fn clear_session_cookie() -> String {
    format!("{SESSION_COOKIE}=; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=0")
}

/// `Set-Cookie` for the anonymous double-submit CSRF cookie (§108). `SameSite=Lax`
/// means a cross-site POST won't carry it, so it can't be validated — the CSRF
/// defense for pre-session requests (login/register/reset).
pub fn set_anon_csrf_cookie(token: &str) -> String {
    format!("{ANON_CSRF_COOKIE}={token}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=3600")
}

/// A fresh random token for an anonymous page, set both as the `csrf` cookie
/// and in the form's hidden field / `<meta name="csrf">` (§108).
pub fn anon_csrf_token() -> String {
    CsrfToken::new(bikenest_infrastructure::RealTokenGenerator.generate()).to_base64url()
}

// ---------------------------------------------------------------------------
// CSRF
// ---------------------------------------------------------------------------

/// Constant-time string comparison (defends against a timing side channel).
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn csrf_cookie_value(headers: &HeaderMap) -> Option<String> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    cookie
        .split(';')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| k.trim() == ANON_CSRF_COOKIE)
        .map(|(_, v)| v.trim().to_string())
}

fn csrf_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// Extract the `csrf` form field from a urlencoded body.
fn csrf_from_form_body(bytes: &[u8], content_type: Option<&str>) -> Option<String> {
    let ct = content_type.unwrap_or("");
    if !ct.starts_with("application/x-www-form-urlencoded") {
        return None;
    }
    for pair in bytes.split(|&b| b == b'&') {
        let eq = pair.iter().position(|&b| b == b'=');
        let (key, value) = match eq {
            Some(i) => (&pair[..i], &pair[i + 1..]),
            None => continue,
        };
        if key == b"csrf" {
            return Some(url_decode(value));
        }
    }
    None
}

fn url_decode(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// The auth middleware: resolve the session cookie into a principal, enforce
/// CSRF on non-GET authenticated requests, and stash the [`Auth`] extension.
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let auth = resolve_auth(&state, req.headers()).await;
    let is_get = req.method() == Method::GET;

    if !is_get {
        // Gather the submitted token: the `X-CSRF-Token` header (htmx reads the
        // <meta name="csrf">) or the `csrf` form field (plain form POSTs).
        let header_token = csrf_from_headers(req.headers());
        let body_token = if header_token.is_none() {
            let content_type = req
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let body = std::mem::replace(req.body_mut(), Body::empty());
            match to_bytes(body, 64 * 1024).await {
                Ok(bytes) => {
                    let tok = csrf_from_form_body(&bytes, content_type.as_deref());
                    // Re-inject the body so the handler's Form extractor can read it.
                    *req.body_mut() = Body::from(bytes);
                    tok
                }
                Err(_) => None,
            }
        } else {
            None
        };
        let submitted = header_token.or(body_token);

        let ok = match (&auth.csrf, submitted.as_deref()) {
            // Authenticated: the per-session synchronizer token (§18).
            (Some(session_csrf), Some(sub)) => session_csrf.verify(sub),
            (Some(_), None) => false,
            // Anonymous: double-submit cookie (§108). A cross-site POST won't
            // carry the `csrf` cookie (SameSite=Lax), so it cannot be validated → 403.
            (None, Some(got)) => match csrf_cookie_value(req.headers()) {
                Some(expected) => constant_time_eq(&expected, got),
                None => false,
            },
            (None, None) => false,
        };
        if !ok {
            return (StatusCode::FORBIDDEN, "Forbidden").into_response();
        }
    }

    req.extensions_mut().insert(auth);
    next.run(req).await
}

async fn resolve_auth(state: &AppState, headers: &HeaderMap) -> Auth {
    let Some(raw) = session_id_from_headers(headers) else {
        return Auth::default();
    };
    let Some(session_id) = SessionId::from_hex(&raw) else {
        return Auth::default();
    };
    match state.auth.resolve_session(&session_id).await {
        Ok(Some(resolved)) => Auth {
            user: Some(resolved.user),
            csrf: Some(resolved.csrf_token),
            session: Some(session_id),
        },
        _ => Auth::default(),
    }
}
