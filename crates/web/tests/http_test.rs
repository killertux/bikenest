//! HTTP-layer tests: M0 health/readiness endpoints + M1 pages.
//!
//! Run via `#[db_test]` so they share the suite's runtime and migrated pool.
//! Page tests that assert against seeded search results use the committed-
//! fixture pattern (see crates/infrastructure/tests/parking_test.rs).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use bikenest_infrastructure::Db;
use bikenest_test_support::{ParkingBuilder, db_test, pool};
use bikenest_web::app_router;
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn test_app() -> axum::Router {
    let db = Db::from_pool(pool().await);
    app_router(db, std::time::Duration::from_secs(2))
}

async fn get(uri: &str) -> (StatusCode, String) {
    let app = test_app().await;
    // Pin the locale to English so assertions on English copy are deterministic
    // (default resolution falls back to pt-BR — §12).
    let res = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("Accept-Language", "en")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).to_string())
}

// ---------------------------------------------------------------------------
// M0: health / readiness
// ---------------------------------------------------------------------------

#[db_test]
async fn healthz_is_alive_without_dependencies(_tx: &mut TestTx) {
    let (status, _) = get("/healthz").await;
    assert_eq!(status, StatusCode::OK);
}

#[db_test]
async fn readyz_returns_ready_with_real_database(_tx: &mut TestTx) {
    let (status, body) = get("/readyz").await;
    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["status"], "ready");
    assert_eq!(json["database"], "up");
}

// ---------------------------------------------------------------------------
// M1: pages
// ---------------------------------------------------------------------------

#[db_test]
async fn home_renders_hero_and_search_form(_tx: &mut TestTx) {
    let (status, body) = get("/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("From destination to parked bike"), "hero headline");
    assert!(body.contains(r#"action="/search""#), "search form");
}

#[db_test]
async fn about_renders_how_it_works(_tx: &mut TestTx) {
    let (status, body) = get("/about").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("How verification"));
}

#[db_test]
async fn search_without_destination_shows_guidance_not_error(_tx: &mut TestTx) {
    let (status, body) = get("/search").await;
    assert_eq!(status, StatusCode::OK, "user input is not a server error");
    assert!(body.contains("Type a destination"));
}

#[db_test]
async fn search_resolves_query_through_fake_geocoder(_tx: &mut TestTx) {
    let (status, body) = get("/search?q=Rua%20XV%20de%20Novembro").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Parking near"), "resolved destination headline");
    assert!(body.contains("search-data"), "map payload embedded");
}

#[db_test]
async fn htmx_request_gets_fragment_without_full_page(_tx: &mut TestTx) {
    let (status, body) = get("/search?lat=-25.4284&lon=-49.2733&sort=distance").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("search-data"));
    // Full-page request vs fragment: send the header this time.
    let app = test_app().await;
    let res = app
        .oneshot(
            Request::builder()
                .uri("/search?lat=-25.4284&lon=-49.2733&sort=distance")
                .header("HX-Request", "true")
                .header("Accept-Language", "en")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&body);
    assert!(!html.contains("<!DOCTYPE"), "fragment must not be a full page");
    assert!(html.contains("search-data"), "fragment still embeds map data");
}

#[db_test]
async fn search_renders_committed_fixture_rows_with_filters(tx: &mut TestTx) {
    const MARK: &str = "fix-http-search";
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
    let conn = tx.executor();
    // Two free racks and one paid locker, ~5.5 km from any other test patch.
    ParkingBuilder::new()
        .with_fixture_tag(MARK)
        .with_name("HTTP Fixture Free A")
        .at(-33.900_000, -70.600_000)
        .create(&mut *conn)
        .await
        .unwrap();
    ParkingBuilder::new()
        .with_fixture_tag(MARK)
        .with_name("HTTP Fixture Free B")
        .at(-33.900_300, -70.600_000)
        .create(&mut *conn)
        .await
        .unwrap();
    ParkingBuilder::new()
        .with_fixture_tag(MARK)
        .with_name("HTTP Fixture Paid")
        .with_cost(bikenest_domain::Cost::Paid { price: None })
        .at(-33.900_600, -70.600_000)
        .create(&mut *conn)
        .await
        .unwrap();
    tx.commit_fixture().await;

    let (_, body) = get("/search?lat=-33.900000&lon=-70.600000&radius=1000&sort=distance").await;
    assert!(body.contains("3 parking spots"), "all fixtures visible: {}", body.len());
    assert!(body.contains("HTTP Fixture Free A"));

    // Cost filter narrows to the free ones.
    let (_, body) = get(
        "/search?lat=-33.900000&lon=-70.600000&radius=1000&sort=distance&cost=free",
    )
    .await;
    assert!(body.contains("2 parking spots"));
    assert!(body.contains("HTTP Fixture Free B"));
    assert!(!body.contains("HTTP Fixture Paid"));

    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
}

#[db_test]
async fn parking_details_renders_full_page_and_404_for_unknown(tx: &mut TestTx) {
    // Unknown id → styled 404 page.
    let (status, body) = get("/parking/999999999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("does not exist"));

    // Self-contained committed fixture (no dependency on `seed-mock`).
    const MARK: &str = "fix-http-details";
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
    let conn = tx.executor();
    let created = ParkingBuilder::new()
        .with_fixture_tag(MARK)
        .with_name("Details Fixture")
        .at(-25.4300, -49.2700)
        .create(&mut *conn)
        .await
        .unwrap();
    tx.commit_fixture().await;

    let (status, body) = get(&format!("/parking/{}", created.id())).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Key facts"));
    assert!(body.contains("Opening hours"));
    assert!(body.contains("Open in Google Maps"), "external navigation (§104)");
    assert!(body.contains("Security attributes"));

    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
}

// ---------------------------------------------------------------------------
// M2: accounts & authentication
// ---------------------------------------------------------------------------

use bikenest_infrastructure::{FakeEmailProvider, FakeOAuthProvider};
use bikenest_test_support::TestPasswordHasher;
use bikenest_web::app_router_with;

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

async fn auth_app() -> (axum::Router, FakeEmailProvider) {
    let email = FakeEmailProvider::with_root(None);
    let oauth = FakeOAuthProvider::new("oauth.user@example.com", "sub-oauth-1");
    let db = Db::from_pool(pool().await);
    let app = app_router_with(
        db,
        std::time::Duration::from_secs(2),
        Box::new(email.clone()),
        oauth,
        Box::new(TestPasswordHasher),
    );
    (app, email)
}

async fn get_c(app: &axum::Router, uri: &str, cookie: Option<&str>) -> (StatusCode, String) {
    let mut b = Request::builder().uri(uri).header("Accept-Language", "en");
    if let Some(c) = cookie {
        b = b.header("cookie", c);
    }
    let res = app.clone().oneshot(b.body(Body::empty()).unwrap()).await.unwrap();
    let status = res.status();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).to_string())
}

/// Which page to GET to obtain the anonymous double-submit CSRF cookie for a
/// given POST route (the form with the hidden `csrf` lives there).
fn anon_source_for(uri: &str) -> Option<&str> {
    if uri.starts_with("/password-reset/new") {
        Some("/password-reset/new")
    } else if uri.starts_with("/password-reset") {
        Some("/password-reset")
    } else if uri.starts_with("/verify-email/resend") {
        Some("/verify-email")
    } else if uri.starts_with("/register") {
        Some("/register")
    } else if uri.starts_with("/login") {
        Some("/login")
    } else {
        None
    }
}

/// GET `page_uri` and return `(csrf cookie line, token)` — the double-submit
/// cookie the CSRF middleware requires on anonymous POSTs.
async fn anon_csrf(app: &axum::Router, page_uri: &str) -> Option<(String, String)> {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(page_uri)
                .header("Accept-Language", "en")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let sc = res
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)?;
    if !sc.starts_with("csrf=") {
        return None;
    }
    let cookie_line = sc.split(';').next().unwrap().to_string();
    let token = cookie_line.split('=').nth(1).unwrap().to_string();
    Some((cookie_line, token))
}

async fn post_form(
    app: &axum::Router,
    uri: &str,
    fields: &[(&str, &str)],
    cookie: Option<&str>,
) -> (StatusCode, String, Option<String>) {
    let mut all_fields: Vec<(String, String)> = fields
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let mut req_cookie = cookie.map(str::to_string);

    // Anonymous POSTs carry the double-submit CSRF cookie + matching form field.
    if cookie.is_none() {
        if let Some(src) = anon_source_for(uri) {
            if let Some((cookie_line, token)) = anon_csrf(app, src).await {
                req_cookie = Some(cookie_line);
                all_fields.push(("csrf".to_string(), token));
            }
        }
    }

    let body = all_fields
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let mut b = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("Accept-Language", "en");
    if let Some(c) = req_cookie {
        b = b.header("cookie", c);
    }
    let res = app
        .clone()
        .oneshot(b.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let set_cookie = res
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let status = res.status();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).to_string(), set_cookie)
}

#[db_test]
async fn register_verify_login_account_logout(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    const EMAIL: &str = "flow@example.com";
    cleanup_user(EMAIL).await;

    // Register → redirect to login?registered=1, verification email captured.
    let (s, _, _) = post_form(&app, "/register", &[("email", EMAIL), ("display_name", "Flow"), ("password", "password123")], None).await;
    assert_eq!(s, StatusCode::SEE_OTHER);
    let token = email.token_for("/verify-email").expect("verification email captured");
    assert!(!token.is_empty());

    // Pending account logs in but /account shows the unverified banner.
    let (s_login, _, cookie) = post_form(&app, "/login", &[("email", EMAIL), ("password", "password123")], None).await;
    assert_eq!(s_login, StatusCode::SEE_OTHER, "login redirects to /account");
    assert!(cookie.as_deref().unwrap_or("").contains("session_id="), "login sets a session cookie");
    let (_, account_before) = get_c(&app, "/account", cookie.as_deref()).await;
    assert!(account_before.contains("Verify your email to contribute"), "unverified banner present");
    assert!(account_before.contains(EMAIL));

    // Verify via the email link, then log in again (verified).
    let (s, _) = get_c(&app, &format!("/verify-email?token={token}"), None).await;
    assert_eq!(s, StatusCode::SEE_OTHER, "verify redirects to login");
    let (_, _, cookie2) = post_form(&app, "/login", &[("email", EMAIL), ("password", "password123")], None).await;
    let cookie = cookie2.unwrap().split(';').next().unwrap().to_string();
    let (_, account_after) = get_c(&app, "/account", Some(&cookie)).await;
    assert!(!account_after.contains("Verify your email to contribute"), "banner gone after verification");

    // Logout clears the session → /account redirects to login.
    let csrf = extract_csrf(&account_after);
    assert!(!csrf.is_empty(), "account page embeds the CSRF token");
    let (s, _, _) = post_form(&app, "/logout", &[("csrf", &csrf)], Some(&cookie)).await;
    assert!(matches!(s, StatusCode::SEE_OTHER | StatusCode::FOUND));
    let (s, _) = get_c(&app, "/account", Some(&cookie)).await;
    assert!(matches!(s, StatusCode::SEE_OTHER | StatusCode::FOUND), "logged-out user is redirected");

    let _ = tx;
    cleanup_user(EMAIL).await;
}

async fn cleanup_user(email: &str) {
    sqlx::query("DELETE FROM users WHERE email = $1")
        .bind(email)
        .execute(&pool().await)
        .await
        .unwrap();
}

#[db_test]
async fn login_failure_body_is_identical_for_unknown_and_existing(tx: &mut bikenest_test_support::TestTx) {
    let (app, _) = auth_app().await;
    cleanup_user("known@example.com").await;
    post_form(&app, "/register", &[("email", "known@example.com"), ("password", "password123")], None).await;

    let (s_known, b_known, _) = post_form(&app, "/login", &[("email", "known@example.com"), ("password", "wrong")], None).await;
    let (s_unknown, b_unknown, _) = post_form(&app, "/login", &[("email", "ghost@example.com"), ("password", "whatever")], None).await;
    assert_eq!(s_known, s_unknown, "same status for known and unknown");
    assert!(b_known.contains("Email or password is incorrect"), "generic message");
    assert!(b_unknown.contains("Email or password is incorrect"));
    // No-account-existence leakage (§45): neither response echoes the submitted
    // email, so an attacker learns nothing from the body. (The pages differ only
    // by the per-request CSRF token, which is unrelated to account existence.)
    assert!(!b_known.contains("known@example.com"));
    assert!(!b_unknown.contains("ghost@example.com"));
    let _ = tx;
    cleanup_user("known@example.com").await;
}

#[db_test]
async fn admin_users_denied_for_anonymous_and_non_admin(tx: &mut bikenest_test_support::TestTx) {
    let (app, _) = auth_app().await;
    cleanup_user("admin-user@example.com").await;

    // Anonymous → redirected to login.
    let (s, _) = get_c(&app, "/admin/users", None).await;
    assert!(matches!(s, StatusCode::SEE_OTHER | StatusCode::FOUND), "anonymous redirected: {s}");

    // Logged-in non-admin → 403.
    post_form(&app, "/register", &[("email", "admin-user@example.com"), ("password", "password123")], None).await;
    let (_, _, cookie) = post_form(&app, "/login", &[("email", "admin-user@example.com"), ("password", "password123")], None).await;
    let (s, _) = get_c(&app, "/admin/users", cookie.as_deref()).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "non-admin forbidden");
    let _ = tx;
    cleanup_user("admin-user@example.com").await;
}

#[db_test]
async fn admin_can_grant_role_and_audit_is_written(tx: &mut bikenest_test_support::TestTx) {
    let (app, _) = auth_app().await;
    cleanup_user("root@example.com").await;
    cleanup_user("target@example.com").await;

    // Seed a logged-in admin + a target user.
    post_form(&app, "/register", &[("email", "root@example.com"), ("password", "password123")], None).await;
    let (_, _, admin_cookie) = post_form(&app, "/login", &[("email", "root@example.com"), ("password", "password123")], None).await;
    let admin_cookie = admin_cookie.unwrap().split(';').next().unwrap().to_string();
    post_form(&app, "/register", &[("email", "target@example.com"), ("password", "password123")], None).await;

    let (root_id,): (i64,) = sqlx::query_as("SELECT id FROM users WHERE email = 'root@example.com'")
        .fetch_one(&pool().await).await.unwrap();
    let (target_id,): (i64,) = sqlx::query_as("SELECT id FROM users WHERE email = 'target@example.com'")
        .fetch_one(&pool().await).await.unwrap();
    sqlx::query("INSERT INTO user_roles (user_id, role, granted_by) VALUES ($1, 'ADMIN', NULL)")
        .bind(root_id).execute(&pool().await).await.unwrap();

    // Admin opens the user list.
    let (s, body) = get_c(&app, "/admin/users", Some(&admin_cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("target@example.com"));

    // Grant MODERATOR. Need the CSRF token from the page.
    let csrf = extract_csrf(&body);
    let (s, _, _) = post_form(&app, &format!("/admin/users/{target_id}/role"),
        &[("csrf", &csrf), ("action", "grant"), ("role", "MODERATOR")], Some(&admin_cookie)).await;
    assert_eq!(s, StatusCode::SEE_OTHER);

    let (audit_count,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM audit_events WHERE action = 'role.granted' AND target_id = $1",
    ).bind(target_id.to_string()).fetch_one(&pool().await).await.unwrap();
    assert_eq!(audit_count, 1, "granted role is audited");

    let (role_count,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM user_roles WHERE user_id = $1 AND role = 'MODERATOR'",
    ).bind(target_id).fetch_one(&pool().await).await.unwrap();
    assert_eq!(role_count, 1);

    let _ = tx;
    cleanup_user("root@example.com").await;
    cleanup_user("target@example.com").await;
}

fn extract_csrf(html: &str) -> String {
    let marker = r#"name="csrf" content=""#;
    let start = html.find(marker).map(|i| i + marker.len()).unwrap_or(0);
    html[start..].split('"').next().unwrap_or("").to_string()
}

#[db_test]
async fn csrf_required_on_authenticated_post(tx: &mut bikenest_test_support::TestTx) {
    let (app, _) = auth_app().await;
    cleanup_user("csrf@example.com").await;
    post_form(&app, "/register", &[("email", "csrf@example.com"), ("password", "password123")], None).await;
    let (_, _, cookie) = post_form(&app, "/login", &[("email", "csrf@example.com"), ("password", "password123")], None).await;
    let cookie = cookie.unwrap().split(';').next().unwrap().to_string();

    // POST /logout with a valid session but NO CSRF token → 403.
    let (s, _, _) = post_form(&app, "/logout", &[], Some(&cookie)).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "missing CSRF token on authenticated POST is forbidden");

    // With the correct CSRF token (from /account) it succeeds.
    let (_, account) = get_c(&app, "/account", Some(&cookie)).await;
    let csrf = extract_csrf(&account);
    let (s, _, _) = post_form(&app, "/logout", &[("csrf", &csrf)], Some(&cookie)).await;
    assert!(matches!(s, StatusCode::SEE_OTHER | StatusCode::FOUND), "logout with CSRF succeeds");
    let _ = tx;
    cleanup_user("csrf@example.com").await;
}

#[db_test]
async fn suspended_account_is_blocked_at_login_with_generic_error(tx: &mut bikenest_test_support::TestTx) {
    let (app, _) = auth_app().await;
    cleanup_user("suspend@example.com").await;
    post_form(&app, "/register", &[("email", "suspend@example.com"), ("password", "password123")], None).await;

    sqlx::query("UPDATE users SET account_state = 'SUSPENDED' WHERE email = 'suspend@example.com'")
        .execute(&pool().await).await.unwrap();

    let (_, body, _) = post_form(&app, "/login", &[("email", "suspend@example.com"), ("password", "password123")], None).await;
    assert!(body.contains("Email or password is incorrect"), "suspended logged out with generic message");
    let _ = tx;
    cleanup_user("suspend@example.com").await;
}

#[db_test]
async fn anonymous_post_without_csrf_cookie_is_forbidden(tx: &mut bikenest_test_support::TestTx) {
    let (app, _) = auth_app().await;
    // A POST with no `csrf` cookie (here the session cookie alone) is rejected on
    // the anonymous path (§108) — SameSite=Lax alone is not treated as CSRF-safe.
    let (s, _, _) = post_form(
        &app,
        "/login",
        &[("email", "x@example.com"), ("password", "password123")],
        Some("session_id=some-invalid-session"),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "anonymous POST without csrf cookie is forbidden");
    let _ = tx;
}

#[db_test]
async fn csrf_header_path_is_accepted(tx: &mut bikenest_test_support::TestTx) {
    let (app, _) = auth_app().await;
    cleanup_user("hdr@example.com").await;
    post_form(&app, "/register", &[("email", "hdr@example.com"), ("password", "password123")], None).await;
    let (_, _, cookie) = post_form(&app, "/login", &[("email", "hdr@example.com"), ("password", "password123")], None).await;
    let cookie = cookie.unwrap().split(';').next().unwrap().to_string();
    let (_, account) = get_c(&app, "/account", Some(&cookie)).await;
    let csrf = extract_csrf(&account);

    // Authenticated POST via the X-CSRF-Token HEADER (the htmx path) is accepted.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/logout")
                .header("cookie", &cookie)
                .header("accept-language", "en")
                .header("x-csrf-token", &csrf)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(res.status(), StatusCode::SEE_OTHER | StatusCode::FOUND), "header-path CSRF accepted");
    let _ = tx;
    cleanup_user("hdr@example.com").await;
}

// ---------------------------------------------------------------------------
// M3 community routes
// ---------------------------------------------------------------------------

async fn cleanup_user_contributions(email: &str) {
    sqlx::query("DELETE FROM parking_location WHERE creator_id = (SELECT id FROM users WHERE email = $1)")
        .bind(email)
        .execute(&pool().await)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = $1")
        .bind(email)
        .execute(&pool().await)
        .await
        .unwrap();
}

async fn unverified_cookie(app: &axum::Router, addr: &str) -> String {
    cleanup_user_contributions(addr).await;
    post_form(app, "/register", &[("email", addr), ("password", "password123")], None).await;
    let (_, _, cookie) = post_form(app, "/login", &[("email", addr), ("password", "password123")], None).await;
    cookie.unwrap().split(';').next().unwrap().to_string()
}

async fn verified_cookie(app: &axum::Router, email: &bikenest_infrastructure::FakeEmailProvider, addr: &str) -> String {
    cleanup_user_contributions(addr).await;
    post_form(app, "/register", &[("email", addr), ("password", "password123")], None).await;
    let token = email.token_for("/verify-email").expect("verification email captured");
    get_c(app, &format!("/verify-email?token={token}"), None).await;
    let (_, _, cookie) = post_form(app, "/login", &[("email", addr), ("password", "password123")], None).await;
    cookie.unwrap().split(';').next().unwrap().to_string()
}

#[db_test]
async fn community_routes_redirect_anonymous(tx: &mut bikenest_test_support::TestTx) {
    let (app, _) = auth_app().await;
    for uri in [
        "/parking/new",
        "/parking/1/edit",
        "/parking/1/review",
        "/account/favorites",
        "/account/contributions",
    ] {
        let (s, _) = get_c(&app, uri, None).await;
        assert!(matches!(s, StatusCode::SEE_OTHER | StatusCode::FOUND), "{uri} redirects anonymous: {s}");
    }
    let _ = tx;
}

#[db_test]
async fn add_location_requires_verified(tx: &mut bikenest_test_support::TestTx) {
    let (app, _) = auth_app().await;
    let cookie = unverified_cookie(&app, "unverified-contrib@example.com").await;
    let (s, _) = get_c(&app, "/parking/new", Some(&cookie)).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "unverified cannot open add form");

    // Even with a forged CSRF, an unverified POST is rejected with 403.
    let (s, _, _) = post_form(&app, "/parking/new",
        &[("csrf", "bogus"), ("name", "X"), ("address", "Y"), ("parking_type", "rack"),
          ("lat", "-25.42"), ("lon", "-49.27"), ("timezone", "America/Sao_Paulo")],
        Some(&cookie)).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "unverified POST denied");
    let _ = tx;
    cleanup_user_contributions("unverified-contrib@example.com").await;
}

#[db_test]
async fn verified_user_adds_a_location_and_sees_details(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    const EMAIL: &str = "contrib-add@example.com";
    let cookie = verified_cookie(&app, &email, EMAIL).await;

    let (s, form) = get_c(&app, "/parking/new", Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK, "verified user opens add form");
    let csrf = extract_csrf(&form);
    assert!(!csrf.is_empty());

    let (s, _, _) = post_form(&app, "/parking/new",
        &[("csrf", &csrf), ("name", "Estação Centro Added"), ("address", "Rua X, 1"),
          ("parking_type", "rack"), ("cost_kind", "unknown"),
          ("lat", "-25.4284"), ("lon", "-49.2733"), ("timezone", "America/Sao_Paulo"),
          ("security", "well_lit")],
        Some(&cookie)).await;
    assert_eq!(s, StatusCode::SEE_OTHER, "add redirects to the new location: {s}");

    // The location is persisted with creator attribution + version 1.
    let (id,): (i64,) = sqlx::query_as(
        "SELECT id FROM parking_location WHERE name = 'Estação Centro Added' ORDER BY id DESC LIMIT 1",
    ).fetch_one(&pool().await).await.unwrap();
    assert!(id > 0);
    let (version,): (i64,) = sqlx::query_as("SELECT version FROM parking_location WHERE id = $1")
        .bind(id).fetch_one(&pool().await).await.unwrap();
    assert_eq!(version, 1);

    // The P3 details page renders.
    let (s, body) = get_c(&app, &format!("/parking/{id}"), Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("Estação Centro Added"));

    let _ = tx;
    cleanup_user_contributions(EMAIL).await;
}

#[db_test]
async fn favorite_toggle_and_list_work_for_authenticated_user(tx: &mut bikenest_test_support::TestTx) {
    const MARK: &str = "fix-http-fav";
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1").bind(MARK)
        .execute(&pool().await).await.unwrap();
    // Fixture location (committed) so the favorite repo can reference it.
    let loc = ParkingBuilder::new().with_name("Favorite Target").with_fixture_tag(MARK)
        .create(tx.executor()).await.unwrap();
    tx.commit_fixture().await;
    let loc_id = loc.id();

    let (app, _) = auth_app().await;
    let cookie = unverified_cookie(&app, "fav-user@example.com").await;

    // GET the details page to grab CSRF, then toggle favorite (auth-only).
    let (s, page) = get_c(&app, &format!("/parking/{loc_id}"), Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    let csrf = extract_csrf(&page);
    let (s, _, _) = post_form(&app, &format!("/parking/{loc_id}/favorite"),
        &[("csrf", &csrf)], Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK, "favorite toggle succeeds for authenticated user");

    // C4 lists the favorited location.
    let (s, body) = get_c(&app, "/account/favorites", Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("Favorite Target"), "favorites page lists the spot");

    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1").bind(MARK)
        .execute(&pool().await).await.unwrap();
    cleanup_user_contributions("fav-user@example.com").await;
}

// ---------------------------------------------------------------------------
// M3: added coverage (plan §9) — edit prefill/data-loss, revision in C5,
// pin-move→PENDING, review aggregate, rate-limit→429, identity absence.
// ---------------------------------------------------------------------------

/// POST /parking/new for a verified session; returns the created location id.
async fn add_location(app: &axum::Router, cookie: &str, csrf: &str, name: &str, extra: &[(&str, &str)]) -> i64 {
    let mut fields: Vec<(String, String)> = vec![
        ("csrf".into(), csrf.into()),
        ("name".into(), name.into()),
        ("address".into(), "Rua X, 1".into()),
        ("parking_type".into(), "rack".into()),
        // Serrra da Cantareira area — well away from the Curitiba seed data.
        ("lat".into(), "-23.4".into()),
        ("lon".into(), "-46.6".into()),
        ("timezone".into(), "America/Sao_Paulo".into()),
    ];
    // cost_kind defaults to unknown; the caller may override it via `extra`.
    if !extra.iter().any(|(k, _)| *k == "cost_kind") {
        fields.push(("cost_kind".into(), "unknown".into()));
    }
    fields.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    let refs: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let (s, _, _) = post_form(app, "/parking/new", &refs, Some(cookie)).await;
    assert_eq!(s, StatusCode::SEE_OTHER, "add should redirect: {s}");
    let (id,): (i64,) = sqlx::query_as("SELECT id FROM parking_location WHERE name = $1 ORDER BY id DESC LIMIT 1")
        .bind(name).fetch_one(&pool().await).await.unwrap();
    id
}

#[db_test]
async fn edit_preserves_cost_security_and_hours(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    const EMAIL: &str = "edit-prefill@example.com";
    let cookie = verified_cookie(&app, &email, EMAIL).await;
    let (_, form) = get_c(&app, "/parking/new", Some(&cookie)).await;
    let csrf = extract_csrf(&form);

    // Create with a paid price + a security attribute + all-day hours.
    let id = add_location(&app, &cookie, &csrf, "Prefill Spot",
        &[("cost_kind", "paid"), ("price", "1"), ("price_currency", "BRL"), ("price_unit", "hour"),
          ("security", "well_lit"), ("open_24h", "true")]).await;

    // The edit page must pre-fill those values.
    let (s, edit_html) = get_c(&app, &format!("/parking/{id}/edit"), Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(edit_html.contains(r#"value="paid" selected"#), "cost pre-filled, not defaulted to free");
    assert!(edit_html.contains("value=\"well_lit\""), "security pre-filled");
    assert!(edit_html.contains("checked"), "open_24h pre-filled");

    // Editing only the name must NOT reset cost/security/hours.
    let edit_csrf = extract_csrf(&edit_html);
    let (s, _, _) = post_form(&app, &format!("/parking/{id}/edit"),
        &[("csrf", &edit_csrf), ("version", "1"), ("name", "Prefill Spot Renamed"),
          ("address", "Rua X, 1"), ("parking_type", "rack"),
          ("cost_kind", "paid"), ("price", "1"), ("price_currency", "BRL"), ("price_unit", "hour"),
          ("security", "well_lit"), ("open_24h", "true")], Some(&cookie)).await;
    assert_eq!(s, StatusCode::SEE_OTHER);

    let (name, cost_kind, price_cents, version): (String, String, Option<i64>, i64) = sqlx::query_as(
        "SELECT name, cost_kind, price_cents, version FROM parking_location WHERE id = $1")
        .bind(id).fetch_one(&pool().await).await.unwrap();
    assert_eq!(name, "Prefill Spot Renamed");
    assert_eq!(cost_kind, "paid", "cost not reset to free");
    assert_eq!(price_cents, Some(100), "price preserved");
    assert_eq!(version, 2, "optimistic bump");
    let (sec,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM parking_security WHERE location_id = $1 AND state = 1 AND feature_code = 'well_lit'")
        .bind(id).fetch_one(&pool().await).await.unwrap();
    assert_eq!(sec, 1, "security preserved");

    let _ = tx;
    cleanup_user_contributions(EMAIL).await;
}

#[db_test]
async fn edit_writes_revision_visible_in_contributions(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    const EMAIL: &str = "edit-c5@example.com";
    let cookie = verified_cookie(&app, &email, EMAIL).await;
    let (_, form) = get_c(&app, "/parking/new", Some(&cookie)).await;
    let csrf = extract_csrf(&form);
    let id = add_location(&app, &cookie, &csrf, "Revision Spot", &[]).await;

    let (_, edit_html) = get_c(&app, &format!("/parking/{id}/edit"), Some(&cookie)).await;
    let edit_csrf = extract_csrf(&edit_html);
    let (s, _, _) = post_form(&app, &format!("/parking/{id}/edit"),
        &[("csrf", &edit_csrf), ("version", "1"), ("name", "Revision Spot 2"),
          ("address", "Rua X, 1"), ("parking_type", "rack"), ("cost_kind", "unknown"),
          ("security", ""), ("open_24h", "")], Some(&cookie)).await;
    assert_eq!(s, StatusCode::SEE_OTHER);

    // The revision row is recorded and C5 shows the edit.
    let (rev,): (i64,) = sqlx::query_as("SELECT count(*) FROM parking_revision WHERE location_id = $1 AND change_kind = 'edit'")
        .bind(id).fetch_one(&pool().await).await.unwrap();
    assert_eq!(rev, 1, "edit revision recorded");
    let (s, body) = get_c(&app, "/account/contributions", Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("Revision Spot 2"), "C5 lists the edited spot");
    assert!(body.contains("Edited"), "C5 shows the edit kind");

    let _ = tx;
    cleanup_user_contributions(EMAIL).await;
}

#[db_test]
async fn proposing_a_move_creates_pending_proposal(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    const EMAIL: &str = "proposal@example.com";
    let cookie = verified_cookie(&app, &email, EMAIL).await;
    let (_, form) = get_c(&app, "/parking/new", Some(&cookie)).await;
    let csrf = extract_csrf(&form);
    let id = add_location(&app, &cookie, &csrf, "Move Spot", &[]).await;

    let (_, edit_html) = get_c(&app, &format!("/parking/{id}/edit"), Some(&cookie)).await;
    let ecsrf = extract_csrf(&edit_html);
    let (s, _, _) = post_form(&app, &format!("/parking/{id}/proposal"),
        &[("csrf", &ecsrf), ("kind", "move_location"), ("lat", "-25.0"), ("lon", "-49.0"), ("timezone", "America/Sao_Paulo"), ("reason", "moved nearby")],
        Some(&cookie)).await;
    assert_eq!(s, StatusCode::SEE_OTHER);

    // A PENDING proposal exists; the location is unchanged (still version 1).
    let (status,): (String,) = sqlx::query_as(
        "SELECT status FROM parking_proposal WHERE location_id = $1 AND kind = 'move_location'")
        .bind(id).fetch_one(&pool().await).await.unwrap();
    assert_eq!(status, "PENDING");
    let (version,): (i64,) = sqlx::query_as("SELECT version FROM parking_location WHERE id = $1")
        .bind(id).fetch_one(&pool().await).await.unwrap();
    assert_eq!(version, 1, "proposal causes no live change");

    // Following the redirect shows the "will be reviewed" confirmation.
    let (s, body) = get_c(&app, &format!("/parking/{id}?proposed=1"), Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("will be reviewed by a moderator"), "proposal confirmation shown");

    let _ = tx;
    cleanup_user_contributions(EMAIL).await;
}

#[db_test]
async fn review_create_updates_aggregate(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    const EMAIL: &str = "review-agg@example.com";
    let cookie = verified_cookie(&app, &email, EMAIL).await;
    let (_, form) = get_c(&app, "/parking/new", Some(&cookie)).await;
    let csrf = extract_csrf(&form);
    let id = add_location(&app, &cookie, &csrf, "Review Spot", &[]).await;

    let (_, review_form) = get_c(&app, &format!("/parking/{id}/review"), Some(&cookie)).await;
    let rcsrf = extract_csrf(&review_form);
    let (s, _, _) = post_form(&app, &format!("/parking/{id}/review"),
        &[("csrf", &rcsrf), ("rating", "4"), ("body", "Great rack")], Some(&cookie)).await;
    assert_eq!(s, StatusCode::SEE_OTHER);

    let (count, avg): (i32, Option<f64>) = sqlx::query_as(
        "SELECT rating_count, rating_avg::float8 FROM parking_location WHERE id = $1")
        .bind(id).fetch_one(&pool().await).await.unwrap();
    assert_eq!(count, 1);
    assert!((avg.unwrap() - 4.0).abs() < 0.001, "rating aggregate updated");
    let (rev,): (i64,) = sqlx::query_as("SELECT count(*) FROM review_revision WHERE review_id IN (SELECT id FROM review WHERE location_id = $1)")
        .bind(id).fetch_one(&pool().await).await.unwrap();
    assert_eq!(rev, 1, "review revision recorded");

    let _ = tx;
    cleanup_user_contributions(EMAIL).await;
}

#[db_test]
async fn parking_create_is_rate_limited(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    const EMAIL: &str = "ratelimit@example.com";
    let cookie = verified_cookie(&app, &email, EMAIL).await;
    let (_, form) = get_c(&app, "/parking/new", Some(&cookie)).await;
    let csrf = extract_csrf(&form);

    // parking-create: 5/day per user (Ledger #6). Post 5, then the 6th → 429.
    // Each add uses a distinct point (and distinct name) so none trips the
    // advisory duplicate check and returns a re-render instead of a redirect.
    for i in 0..5 {
        let lat = -23.4 - i as f64 * 0.01;
        let name = format!("RateSpot{n}{i}", n = i); // distinct low-similarity names
        let (s, _, _) = post_form(&app, "/parking/new",
            &[("csrf", &csrf), ("name", &name), ("address", "Rua X, 1"),
              ("parking_type", "rack"), ("cost_kind", "unknown"), ("lat", &lat.to_string()),
              ("lon", "-46.6"), ("timezone", "America/Sao_Paulo")], Some(&cookie)).await;
        assert_eq!(s, StatusCode::SEE_OTHER, "add {i} should succeed");
    }
    let (s, _, _) = post_form(&app, "/parking/new",
        &[("csrf", &csrf), ("name", "RateSpotFinal"), ("address", "Rua X, 1"),
          ("parking_type", "rack"), ("cost_kind", "unknown"), ("lat", "-23.4"),
          ("lon", "-46.6"), ("timezone", "America/Sao_Paulo")], Some(&cookie)).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "6th add is rate-limited");

    let _ = tx;
    cleanup_user_contributions(EMAIL).await;
}

#[db_test]
async fn no_identity_leak_in_rendered_html(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    const EMAIL: &str = "identity@example.com";
    let cookie = verified_cookie(&app, &email, EMAIL).await;
    let (_, form) = get_c(&app, "/parking/new", Some(&cookie)).await;
    let csrf = extract_csrf(&form);
    let id = add_location(&app, &cookie, &csrf, "Identity Spot", &[]).await;

    let (user_id,): (i64,) = sqlx::query_as("SELECT id FROM users WHERE email = $1")
        .bind(EMAIL).fetch_one(&pool().await).await.unwrap();

    // The details page, favorite/verify state and C5 must never render the
    // contributor's email, user id, or the OAuth subject (only counts/labels).
    let uris: Vec<String> = vec![
        format!("/parking/{id}"),
        "/account/contributions".to_string(),
        "/account/favorites".to_string(),
    ];
    for uri in uris {
        let (s, body) = get_c(&app, &uri, Some(&cookie)).await;
        assert_eq!(s, StatusCode::OK);
        assert!(!body.contains(EMAIL), "email leaked on {uri}");
        assert!(!body.contains(&format!(">{user_id}<")), "user id leaked on {uri}");
    }

    let _ = tx;
    cleanup_user_contributions(EMAIL).await;
}

#[db_test]
async fn multiple_security_values_and_major_unit_price(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    const EMAIL: &str = "multi-sec@example.com";
    const MARK: &str = "fix-http-multisec";
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1").bind(MARK)
        .execute(&pool().await).await.unwrap();
    let cookie = verified_cookie(&app, &email, EMAIL).await;
    let (_, form) = get_c(&app, "/parking/new", Some(&cookie)).await;
    let csrf = extract_csrf(&form);

    // TWO security attributes (the source of the "duplicate field" crash) plus a
    // major-unit price ("1.50" must store as 150 cents, not raw text/cents).
    let id = add_location(&app, &cookie, &csrf, "Multi Security Spot",
        &[("cost_kind", "paid"), ("price", "1.50"), ("price_currency", "BRL"), ("price_unit", "hour"),
          ("security", "well_lit,cctv")]).await;

    let (cost_kind, price_cents, version): (String, Option<i64>, i64) =
        sqlx::query_as("SELECT cost_kind, price_cents, version FROM parking_location WHERE id = $1")
            .bind(id).fetch_one(&pool().await).await.unwrap();
    assert_eq!(cost_kind, "paid");
    assert_eq!(price_cents, Some(150), "1.50 majors -> 150 cents");
    assert_eq!(version, 1);

    // Both security attributes recorded as YES.
    let (yes,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM parking_security WHERE location_id = $1 AND state = 1 AND feature_code IN ('well_lit','cctv')")
        .bind(id).fetch_one(&pool().await).await.unwrap();
    assert_eq!(yes, 2, "both security attributes stored");

    // Editing preserves both (no reset) and the price round-trips as majors.
    let (_, edit_html) = get_c(&app, &format!("/parking/{id}/edit"), Some(&cookie)).await;
    let ecsrf = extract_csrf(&edit_html);
    let (s, _, _) = post_form(&app, &format!("/parking/{id}/edit"),
        &[("csrf", &ecsrf), ("version", "1"), ("name", "Multi Security Spot"),
          ("address", "Rua X, 1"), ("parking_type", "rack"),
          ("cost_kind", "paid"), ("price", "1.50"), ("price_currency", "BRL"), ("price_unit", "hour"),
          ("security", "well_lit,cctv"), ("open_24h", "")], Some(&cookie)).await;
    assert_eq!(s, StatusCode::SEE_OTHER);
    let (yes,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM parking_security WHERE location_id = $1 AND state = 1 AND feature_code IN ('well_lit','cctv')")
        .bind(id).fetch_one(&pool().await).await.unwrap();
    assert_eq!(yes, 2, "security preserved through edit");

    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1").bind(MARK)
        .execute(&pool().await).await.unwrap();
    cleanup_user_contributions(EMAIL).await;
}
