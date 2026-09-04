//! HTTP-layer tests: M0 health/readiness endpoints + M1 pages.
//!
//! Run via `#[db_test]` so they share the suite's runtime and migrated pool.
//! Page tests that assert against seeded search results use the committed-
//! fixture pattern (see crates/infrastructure/tests/parking_test.rs).

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use bikenest_infrastructure::Db;
use bikenest_test_support::{ParkingBuilder, db_test, pool};
use bikenest_web::app_router;
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn test_app() -> axum::Router {
    let db = Db::from_pool(pool().await);
    app_router(db, std::time::Duration::from_secs(2))
}

/// GET and return only the response headers (for security-header asserts).
async fn get_headers(uri: &str) -> HeaderMap {
    let app = test_app().await;
    let res = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    res.headers().clone()
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
// Security headers (§64/§65)
// ---------------------------------------------------------------------------

#[db_test]
async fn security_headers_present_on_public_page(_tx: &mut TestTx) {
    let headers = get_headers("/").await;
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(
        headers["referrer-policy"],
        "strict-origin-when-cross-origin"
    );
    assert_eq!(headers["x-frame-options"], "DENY");
    assert!(headers.contains_key("content-security-policy"));
    assert!(headers.contains_key("permissions-policy"));
}

#[db_test]
async fn security_headers_present_on_private_page(_tx: &mut TestTx) {
    // Account page redirects anonymous users, but the header set rides every response.
    let headers = get_headers("/login").await;
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert!(headers.contains_key("content-security-policy"));
}

#[db_test]
async fn security_headers_present_when_auth_short_circuits(_tx: &mut TestTx) {
    // A state-changing request without a CSRF cookie → the auth middleware returns
    // 403 *without* running the inner handler. The security-header middleware is
    // outermost and must still apply CSP/nosniff to that response (§64/§65).
    let app = test_app().await;
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/logout")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert!(
        res.headers().contains_key("content-security-policy"),
        "CSP must be present on a CSRF-403 response"
    );
    assert_eq!(res.headers()["x-content-type-options"], "nosniff");
}

#[db_test]
async fn csp_is_strict_no_unsafe_eval(_tx: &mut TestTx) {
    let headers = get_headers("/").await;
    let csp = headers["content-security-policy"]
        .to_str()
        .unwrap()
        .to_string();
    assert!(csp.starts_with("default-src 'self'"), "csp: {csp}");
    assert!(csp.contains("script-src 'self'"), "csp: {csp}");
    assert!(!csp.contains("unsafe-eval"), "csp must forbid eval: {csp}");
    assert!(csp.contains("object-src 'none'"), "csp: {csp}");
    assert!(csp.contains("frame-ancestors 'none'"), "csp: {csp}");
}

// `img-src` honoring `CSP_MEDIA_HOSTS` is covered by
// `security::tests::csp_img_src_includes_media_hosts` (crates/web/src/security.rs),
// which builds `SecurityHeaders` directly instead of mutating the process
// environment for the duration of a live request (now that workspace lints —
// including `unsafe_code = "forbid"` — are enforced on this crate, an
// unguarded `std::env::set_var`/`remove_var` pair no longer compiles here).

#[db_test]
async fn hsts_absent_in_dev_without_tls(_tx: &mut TestTx) {
    // `TLS_ON` is unset in the test environment → no HSTS header.
    let headers = get_headers("/").await;
    assert!(!headers.contains_key("strict-transport-security"));
}

// ---------------------------------------------------------------------------
// SEO / indexing (§109/§110/§111)
// ---------------------------------------------------------------------------

#[db_test]
async fn robots_txt_matches_crawl_policy(_tx: &mut TestTx) {
    let (status, body) = get("/robots.txt").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("User-agent: *"));
    assert!(body.contains("Allow: /"));
    assert!(body.contains("Disallow: /account"));
    assert!(body.contains("Disallow: /admin"));
    assert!(body.contains("Disallow: /moderation"));
}

#[db_test]
async fn sitemap_includes_static_pages_and_active_parking(tx: &mut TestTx) {
    const MARK: &str = "fix-http-sitemap";
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
    let conn = tx.executor();
    let loc = ParkingBuilder::new()
        .with_fixture_tag(MARK)
        .with_name("Sitemap Fixture")
        .at(-33.920_000, -70.620_000)
        .create(&mut *conn)
        .await
        .unwrap();
    tx.commit_fixture().await;

    let (status, body) = get("/sitemap.xml").await;
    assert_eq!(status, StatusCode::OK);
    let base = "http://localhost:8080";
    assert!(
        body.contains(&format!("<loc>{base}/about</loc>")),
        "static page in sitemap"
    );
    assert!(
        body.contains(&format!("<loc>{base}/search</loc>")),
        "static page in sitemap"
    );
    assert!(
        body.contains(&format!("<loc>{base}/parking/{}</loc>", loc.id())),
        "active parking in sitemap"
    );

    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
}

#[db_test]
async fn public_page_has_canonical_description_and_hreflang(_tx: &mut TestTx) {
    let (status, body) = get("/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("rel=\"canonical\""),
        "home has canonical link"
    );
    assert!(
        body.contains("name=\"description\""),
        "home has meta description"
    );
    assert!(body.contains("property=\"og:title\""), "home has og:title");
    assert!(
        body.contains("hreflang=\"pt-BR\""),
        "hreflang pt-BR present"
    );
    assert!(body.contains("hreflang=\"en\""), "hreflang en present");
}

#[db_test]
async fn private_pages_are_noindex(_tx: &mut TestTx) {
    for path in ["/account", "/admin/users", "/moderation"] {
        let headers = get_headers(path).await;
        let v = headers["x-robots-tag"].to_str().unwrap();
        assert!(v.contains("noindex"), "{path} must be noindex, got: {v}");
    }
}

// ---------------------------------------------------------------------------
// M1: pages
// ---------------------------------------------------------------------------

#[db_test]
async fn home_renders_hero_and_search_form(_tx: &mut TestTx) {
    let (status, body) = get("/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("From destination to parked bike"),
        "hero headline"
    );
    assert!(body.contains(r#"action="/search""#), "search form");
}

#[db_test]
async fn skip_link_targets_main_content(_tx: &mut TestTx) {
    let (status, body) = get("/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r##"href="#content""##), "skip link");
    assert!(body.contains(r#"id="content""#), "main landmark");
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
    assert!(
        body.contains("Parking near"),
        "resolved destination headline"
    );
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
    assert!(
        !html.contains("<!DOCTYPE"),
        "fragment must not be a full page"
    );
    assert!(
        html.contains("search-data"),
        "fragment still embeds map data"
    );
}

#[db_test]
async fn htmx_search_fragment_updates_result_count_out_of_band(_tx: &mut TestTx) {
    // The result count / destination heading live outside `#results` in
    // search.html, so the fragment response must carry `hx-swap-oob` copies
    // for htmx to patch them in place.
    let app = test_app().await;
    let res = app
        .oneshot(
            Request::builder()
                .uri("/search?q=x")
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
    assert!(html.contains(r#"id="result-count""#), "fragment: {html}");
    assert!(html.contains("hx-swap-oob"), "fragment: {html}");
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
    assert!(
        body.contains("3 parking spots"),
        "all fixtures visible: {}",
        body.len()
    );
    assert!(body.contains("HTTP Fixture Free A"));

    // Cost filter narrows to the free ones.
    let (_, body) =
        get("/search?lat=-33.900000&lon=-70.600000&radius=1000&sort=distance&cost=free").await;
    assert!(body.contains("2 parking spots"));
    assert!(body.contains("HTTP Fixture Free B"));
    assert!(!body.contains("HTTP Fixture Paid"));

    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
}

/// Stored-XSS regression (§103): a user-controlled `name`/`address` containing
/// `</script>` must not break out of the `<script type="application/json"
/// id="search-data">` embed. The map payload escapes `<`/`>`/`&`/U+2028/U+2029
/// so `JSON.parse` round-trips the original value but no literal `</script>`
/// survives.
#[db_test]
async fn search_map_payload_is_html_safe_for_ugc_names(tx: &mut TestTx) {
    const MARK: &str = "fix-http-xss";
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
    let conn = tx.executor();
    let payload_attack = "</script><img src=x onerror=alert(1)>";
    ParkingBuilder::new()
        .with_fixture_tag(MARK)
        .with_name(payload_attack)
        .at(-33.910_000, -70.610_000)
        .create(&mut *conn)
        .await
        .unwrap();
    tx.commit_fixture().await;

    let (_, body) = get("/search?lat=-33.910000&lon=-70.610000&radius=1000&sort=distance").await;

    // The escaped JSON must be present, so the browser's JSON.parse gets `<` back.
    assert!(
        body.contains(r"\u003c/script\u003e"),
        "map payload must escape the closing script tag"
    );
    // The raw contiguous attack sequence must be gone from the whole response.
    assert!(
        !body.contains(payload_attack),
        "raw attack sequence must not appear"
    );
    // Grab the search-data block and parse it back — the original value survives.
    let marker = "<script type=\"application/json\" id=\"search-data\">";
    let start = body.find(marker).expect("search-data block present");
    let rest = &body[start + marker.len()..];
    let end = rest.find("</script>").unwrap_or(rest.len());
    let json_block = &rest[..end];
    assert!(json_block.contains(r"\u003cimg src=x onerror=alert(1)\u003e"));
    let parsed: serde_json::Value = serde_json::from_str(json_block).expect("valid JSON block");
    let round_trip = serde_json::to_string(&parsed).unwrap();
    assert!(
        round_trip.contains(payload_attack),
        "JSON.parse must round-trip to the original value"
    );

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
    assert!(
        body.contains("Open in Google Maps"),
        "external navigation (§104)"
    );
    assert!(body.contains("Security attributes"));

    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
}

#[db_test]
async fn never_verified_location_shows_the_freshness_label_once(tx: &mut TestTx) {
    // Problem #4 regression: a never-verified location's freshness card must
    // not render the "never verified" copy twice — `freshness_label` and
    // `verified_label` both resolve to the same string when there's no
    // `last_verified_at`, so the template must collapse them into one.
    const MARK: &str = "fix-http-never-verified";
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
    let conn = tx.executor();
    let created = ParkingBuilder::new()
        .with_fixture_tag(MARK)
        .with_name("Never Verified Fixture")
        .at(-25.4300, -49.2700)
        .never_verified()
        .create(&mut *conn)
        .await
        .unwrap();
    tx.commit_fixture().await;

    let (status, body) = get(&format!("/parking/{}", created.id())).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !body.contains("Never verified · Never verified"),
        "freshness card must not repeat the never-verified label"
    );
    assert!(
        body.contains("Never verified"),
        "freshness card should still show the label once"
    );

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
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

async fn auth_app() -> (axum::Router, FakeEmailProvider) {
    // The wider M2/M3/... suite exercises the fake OAuth flow's plumbing, so
    // it builds with Google sign-in enabled; WP1's own tests below build with
    // `auth_app_opts(false)` to cover the disabled (default) product state.
    auth_app_opts(true).await
}

/// Like [`auth_app`], but with the Google sign-in feature flag set explicitly
/// (product decision: disabled by default until a real OAuth provider exists).
async fn auth_app_opts(google_oauth_enabled: bool) -> (axum::Router, FakeEmailProvider) {
    let email = FakeEmailProvider::with_root(None);
    let oauth = FakeOAuthProvider::new("oauth.user@example.com", "sub-oauth-1");
    let db = Db::from_pool(pool().await);
    let app = app_router_with(
        db,
        std::time::Duration::from_secs(2),
        Box::new(email.clone()),
        oauth,
        TestPasswordHasher,
        Box::new(bikenest_infrastructure::InMemoryRateLimiter::new()),
        std::sync::Arc::new(bikenest_test_support::TestObjectStorage::new()),
        google_oauth_enabled,
    );
    (app, email)
}

async fn get_c(app: &axum::Router, uri: &str, cookie: Option<&str>) -> (StatusCode, String) {
    let mut b = Request::builder().uri(uri).header("Accept-Language", "en");
    if let Some(c) = cookie {
        b = b.header("cookie", c);
    }
    let res = app
        .clone()
        .oneshot(b.body(Body::empty()).unwrap())
        .await
        .unwrap();
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
    if cookie.is_none()
        && let Some(src) = anon_source_for(uri)
        && let Some((cookie_line, token)) = anon_csrf(app, src).await
    {
        req_cookie = Some(cookie_line);
        all_fields.push(("csrf".to_string(), token));
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
    (
        status,
        String::from_utf8_lossy(&body).to_string(),
        set_cookie,
    )
}

#[db_test]
async fn register_verify_login_account_logout(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    const EMAIL: &str = "flow@example.com";
    cleanup_user(EMAIL).await;

    // Register → redirect to login?registered=1, verification email captured.
    let (s, _, _) = post_form(
        &app,
        "/register",
        &[
            ("email", EMAIL),
            ("display_name", "Flow"),
            ("password", "password123"),
        ],
        None,
    )
    .await;
    assert_eq!(s, StatusCode::SEE_OTHER);
    let token = email
        .token_for("/verify-email")
        .expect("verification email captured");
    assert!(!token.is_empty());

    // Pending account logs in but /account shows the unverified banner.
    let (s_login, _, cookie) = post_form(
        &app,
        "/login",
        &[("email", EMAIL), ("password", "password123")],
        None,
    )
    .await;
    assert_eq!(
        s_login,
        StatusCode::SEE_OTHER,
        "login redirects to /account"
    );
    assert!(
        cookie.as_deref().unwrap_or("").contains("session_id="),
        "login sets a session cookie"
    );
    let (_, account_before) = get_c(&app, "/account", cookie.as_deref()).await;
    assert!(
        account_before.contains("Verify your email to contribute"),
        "unverified banner present"
    );
    assert!(account_before.contains(EMAIL));

    // Verify via the email link, then log in again (verified).
    let (s, _) = get_c(&app, &format!("/verify-email?token={token}"), None).await;
    assert_eq!(s, StatusCode::SEE_OTHER, "verify redirects to login");
    let (_, _, cookie2) = post_form(
        &app,
        "/login",
        &[("email", EMAIL), ("password", "password123")],
        None,
    )
    .await;
    let cookie = cookie2.unwrap().split(';').next().unwrap().to_string();
    let (_, account_after) = get_c(&app, "/account", Some(&cookie)).await;
    assert!(
        !account_after.contains("Verify your email to contribute"),
        "banner gone after verification"
    );

    // Logout clears the session → /account redirects to login.
    let csrf = extract_csrf(&account_after);
    assert!(!csrf.is_empty(), "account page embeds the CSRF token");
    let (s, _, _) = post_form(&app, "/logout", &[("csrf", &csrf)], Some(&cookie)).await;
    assert!(matches!(s, StatusCode::SEE_OTHER | StatusCode::FOUND));
    let (s, _) = get_c(&app, "/account", Some(&cookie)).await;
    assert!(
        matches!(s, StatusCode::SEE_OTHER | StatusCode::FOUND),
        "logged-out user is redirected"
    );

    let _ = tx;
    cleanup_user(EMAIL).await;
}

#[db_test]
async fn resend_verification_from_account_page_succeeds(tx: &mut bikenest_test_support::TestTx) {
    let (app, _email) = auth_app().await;
    const EMAIL: &str = "resend-account@example.com";
    let cookie = unverified_cookie(&app, EMAIL).await;

    let (_, account_body) = get_c(&app, "/account", Some(&cookie)).await;
    assert!(
        account_body.contains("Verify your email to contribute"),
        "unverified banner present"
    );
    let csrf = extract_csrf(&account_body);
    assert!(!csrf.is_empty(), "account page embeds the CSRF token");

    // The account-page resend form now carries the session's CSRF token
    // (previously missing, which made this authenticated POST fail CSRF with
    // 403 instead of succeeding like the anonymous /verify-email form).
    let (s, _, _) = post_form(
        &app,
        "/verify-email/resend",
        &[("csrf", &csrf), ("email", EMAIL)],
        Some(&cookie),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::SEE_OTHER,
        "resend-verification POST from /account must succeed, not 403"
    );

    let _ = tx;
    cleanup_user_contributions(EMAIL).await;
}

#[db_test]
async fn privacy_public_pages_gating_and_export_flow(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    const EMAIL: &str = "privacy-web@example.com";
    cleanup_user(EMAIL).await;

    // Public legal pages render (200), even with placeholder content.
    let (s, body) = get_c(&app, "/privacy", None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("Privacy policy"));
    let (s, _) = get_c(&app, "/terms", None).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = get_c(&app, "/cookies", None).await;
    assert_eq!(s, StatusCode::OK);

    // §71: the sign-up form links the terms + privacy policy next to the button.
    let (s, body) = get_c(&app, "/register", None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        body.contains("href=\"/terms\"") && body.contains("href=\"/privacy\""),
        "register page must link the policies"
    );
    assert!(body.contains("18 or older"));

    // Authenticated+admin surfaces redirect anonymous users to login.
    let (s, _) = get_c(&app, "/account/privacy", None).await;
    assert!(
        matches!(s, StatusCode::SEE_OTHER | StatusCode::FOUND),
        "privacy hub requires auth"
    );
    let (s, _) = get_c(&app, "/admin/privacy-requests", None).await;
    assert!(
        matches!(s, StatusCode::SEE_OTHER | StatusCode::FOUND),
        "admin queue requires login"
    );

    // Register -> verify -> login.
    post_form(
        &app,
        "/register",
        &[("email", EMAIL), ("password", "password123")],
        None,
    )
    .await;
    let token = email
        .token_for("/verify-email")
        .expect("verification email captured");
    get_c(&app, &format!("/verify-email?token={token}"), None).await;
    let (_, _, cookie) = post_form(
        &app,
        "/login",
        &[("email", EMAIL), ("password", "password123")],
        None,
    )
    .await;
    let cookie = cookie.unwrap().split(';').next().unwrap().to_string();

    // C6 hub renders with a CSRF token.
    let (s, body) = get_c(&app, "/account/privacy", Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("Export your data"));
    let csrf = extract_csrf(&body);

    // Request an export: POST -> 303 redirect to /account/export/{id}?token=...
    let req = Request::builder()
        .method("POST")
        .uri("/account/privacy/export")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("cookie", &cookie)
        .header("Accept-Language", "en")
        .body(Body::from(format!("csrf={csrf}")))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::SEE_OTHER,
        "export request redirects"
    );
    let loc = res
        .headers()
        .get(axum::http::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        loc.starts_with("/account/export/"),
        "redirect carries the export link"
    );

    // The C7 page renders the export status (Ready) and — the id now comes
    // from the path, not a query param that was never set — the single-use
    // download link.
    let (s, body) = get_c(&app, &loc, Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("Ready") || body.contains("Downloaded") || body.contains("Expired"));
    let (export_path, query) = loc.split_once('?').unwrap_or((&loc, ""));
    assert!(
        body.contains(&format!("{export_path}/download")),
        "export page must render the download link: {body}"
    );

    // The single-use download returns JSON with attachment headers.
    let download_uri = format!("{export_path}/download?{query}");
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&download_uri)
                .header("cookie", &cookie)
                .header("Accept-Language", "en")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "download succeeds for the owner"
    );
    assert_eq!(
        res.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/json; charset=utf-8"),
    );

    // A non-admin (this user is USER only) gets 403 on the admin queue.
    let (s, _) = get_c(&app, "/admin/privacy-requests", Some(&cookie)).await;
    assert_eq!(
        s,
        StatusCode::FORBIDDEN,
        "non-admin blocked from the privacy-request queue"
    );

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
async fn login_failure_body_is_identical_for_unknown_and_existing(
    tx: &mut bikenest_test_support::TestTx,
) {
    let (app, _) = auth_app().await;
    cleanup_user("known@example.com").await;
    post_form(
        &app,
        "/register",
        &[("email", "known@example.com"), ("password", "password123")],
        None,
    )
    .await;

    let (s_known, b_known, _) = post_form(
        &app,
        "/login",
        &[("email", "known@example.com"), ("password", "wrong")],
        None,
    )
    .await;
    let (s_unknown, b_unknown, _) = post_form(
        &app,
        "/login",
        &[("email", "ghost@example.com"), ("password", "whatever")],
        None,
    )
    .await;
    assert_eq!(s_known, s_unknown, "same status for known and unknown");
    assert!(
        b_known.contains("Email or password is incorrect"),
        "generic message"
    );
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
    assert!(
        matches!(s, StatusCode::SEE_OTHER | StatusCode::FOUND),
        "anonymous redirected: {s}"
    );

    // Logged-in non-admin → 403.
    post_form(
        &app,
        "/register",
        &[
            ("email", "admin-user@example.com"),
            ("password", "password123"),
        ],
        None,
    )
    .await;
    let (_, _, cookie) = post_form(
        &app,
        "/login",
        &[
            ("email", "admin-user@example.com"),
            ("password", "password123"),
        ],
        None,
    )
    .await;
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
    post_form(
        &app,
        "/register",
        &[("email", "root@example.com"), ("password", "password123")],
        None,
    )
    .await;
    let (_, _, admin_cookie) = post_form(
        &app,
        "/login",
        &[("email", "root@example.com"), ("password", "password123")],
        None,
    )
    .await;
    let admin_cookie = admin_cookie.unwrap().split(';').next().unwrap().to_string();
    post_form(
        &app,
        "/register",
        &[("email", "target@example.com"), ("password", "password123")],
        None,
    )
    .await;

    let (root_id,): (i64,) =
        sqlx::query_as("SELECT id FROM users WHERE email = 'root@example.com'")
            .fetch_one(&pool().await)
            .await
            .unwrap();
    let (target_id,): (i64,) =
        sqlx::query_as("SELECT id FROM users WHERE email = 'target@example.com'")
            .fetch_one(&pool().await)
            .await
            .unwrap();
    sqlx::query("INSERT INTO user_roles (user_id, role, granted_by) VALUES ($1, 'ADMIN', NULL)")
        .bind(root_id)
        .execute(&pool().await)
        .await
        .unwrap();

    // Admin opens the user list.
    let (s, body) = get_c(&app, "/admin/users", Some(&admin_cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("target@example.com"));

    // Grant MODERATOR. Need the CSRF token from the page.
    let csrf = extract_csrf(&body);
    let (s, _, _) = post_form(
        &app,
        &format!("/admin/users/{target_id}/role"),
        &[("csrf", &csrf), ("action", "grant"), ("role", "MODERATOR")],
        Some(&admin_cookie),
    )
    .await;
    assert_eq!(s, StatusCode::SEE_OTHER);

    let (audit_count,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM audit_events WHERE action = 'role.granted' AND target_id = $1",
    )
    .bind(target_id.to_string())
    .fetch_one(&pool().await)
    .await
    .unwrap();
    assert_eq!(audit_count, 1, "granted role is audited");

    let (role_count,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM user_roles WHERE user_id = $1 AND role = 'MODERATOR'")
            .bind(target_id)
            .fetch_one(&pool().await)
            .await
            .unwrap();
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
    post_form(
        &app,
        "/register",
        &[("email", "csrf@example.com"), ("password", "password123")],
        None,
    )
    .await;
    let (_, _, cookie) = post_form(
        &app,
        "/login",
        &[("email", "csrf@example.com"), ("password", "password123")],
        None,
    )
    .await;
    let cookie = cookie.unwrap().split(';').next().unwrap().to_string();

    // POST /logout with a valid session but NO CSRF token → 403.
    let (s, _, _) = post_form(&app, "/logout", &[], Some(&cookie)).await;
    assert_eq!(
        s,
        StatusCode::FORBIDDEN,
        "missing CSRF token on authenticated POST is forbidden"
    );

    // With the correct CSRF token (from /account) it succeeds.
    let (_, account) = get_c(&app, "/account", Some(&cookie)).await;
    let csrf = extract_csrf(&account);
    let (s, _, _) = post_form(&app, "/logout", &[("csrf", &csrf)], Some(&cookie)).await;
    assert!(
        matches!(s, StatusCode::SEE_OTHER | StatusCode::FOUND),
        "logout with CSRF succeeds"
    );
    let _ = tx;
    cleanup_user("csrf@example.com").await;
}

#[db_test]
async fn suspended_account_is_blocked_at_login_with_generic_error(
    tx: &mut bikenest_test_support::TestTx,
) {
    let (app, _) = auth_app().await;
    cleanup_user("suspend@example.com").await;
    post_form(
        &app,
        "/register",
        &[
            ("email", "suspend@example.com"),
            ("password", "password123"),
        ],
        None,
    )
    .await;

    sqlx::query("UPDATE users SET account_state = 'SUSPENDED' WHERE email = 'suspend@example.com'")
        .execute(&pool().await)
        .await
        .unwrap();

    let (_, body, _) = post_form(
        &app,
        "/login",
        &[
            ("email", "suspend@example.com"),
            ("password", "password123"),
        ],
        None,
    )
    .await;
    assert!(
        body.contains("Email or password is incorrect"),
        "suspended logged out with generic message"
    );
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
    assert_eq!(
        s,
        StatusCode::FORBIDDEN,
        "anonymous POST without csrf cookie is forbidden"
    );
    let _ = tx;
}

#[db_test]
async fn csrf_header_path_is_accepted(tx: &mut bikenest_test_support::TestTx) {
    let (app, _) = auth_app().await;
    cleanup_user("hdr@example.com").await;
    post_form(
        &app,
        "/register",
        &[("email", "hdr@example.com"), ("password", "password123")],
        None,
    )
    .await;
    let (_, _, cookie) = post_form(
        &app,
        "/login",
        &[("email", "hdr@example.com"), ("password", "password123")],
        None,
    )
    .await;
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
    assert!(
        matches!(res.status(), StatusCode::SEE_OTHER | StatusCode::FOUND),
        "header-path CSRF accepted"
    );
    let _ = tx;
    cleanup_user("hdr@example.com").await;
}

// ---------------------------------------------------------------------------
// WP1: Google sign-in disabled by default (product decision: disable, do not
// implement real OAuth). `FakeOAuthProvider` still exists for opt-in dev use,
// but the routes are unregistered unless `GOOGLE_OAUTH_ENABLED` is set.
// ---------------------------------------------------------------------------

#[db_test]
async fn google_oauth_disabled_by_default_returns_404(_tx: &mut TestTx) {
    let app = test_app().await;
    let res = app
        .oneshot(
            Request::builder()
                .uri("/auth/google")
                .header("Accept-Language", "en")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(
        res.headers().get("set-cookie").is_none(),
        "a disabled/unregistered route must not set any cookie"
    );
}

#[db_test]
async fn google_oauth_disabled_callback_returns_404(_tx: &mut TestTx) {
    let (status, _) = get("/auth/google/callback?code=x&state=y").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[db_test]
async fn login_page_hides_google_link_when_disabled(_tx: &mut TestTx) {
    let (status, body) = get("/login").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !body.contains("href=\"/auth/google\""),
        "no live Google link when the feature is disabled"
    );
    assert!(
        body.contains("Coming soon"),
        "disabled button shows the coming-soon copy"
    );
}

#[db_test]
async fn google_oauth_enabled_flag_still_redirects(_tx: &mut TestTx) {
    // Protects the future real-integration plumbing: with the flag explicitly
    // on, `/auth/google` is registered and behaves as before (redirect to the
    // provider's authorize URL — the fake's consent stub in this test build).
    let (app, _) = auth_app_opts(true).await;
    let (status, _) = get_c(&app, "/auth/google", None).await;
    assert!(
        matches!(
            status,
            StatusCode::SEE_OTHER | StatusCode::FOUND | StatusCode::TEMPORARY_REDIRECT
        ),
        "enabled Google route redirects, got {status}"
    );
}

// ---------------------------------------------------------------------------
// M3 community routes
// ---------------------------------------------------------------------------

async fn cleanup_user_contributions(email: &str) {
    sqlx::query(
        "DELETE FROM parking_location WHERE creator_id = (SELECT id FROM users WHERE email = $1)",
    )
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
    post_form(
        app,
        "/register",
        &[("email", addr), ("password", "password123")],
        None,
    )
    .await;
    let (_, _, cookie) = post_form(
        app,
        "/login",
        &[("email", addr), ("password", "password123")],
        None,
    )
    .await;
    cookie.unwrap().split(';').next().unwrap().to_string()
}

async fn verified_cookie(
    app: &axum::Router,
    email: &bikenest_infrastructure::FakeEmailProvider,
    addr: &str,
) -> String {
    cleanup_user_contributions(addr).await;
    post_form(
        app,
        "/register",
        &[("email", addr), ("password", "password123")],
        None,
    )
    .await;
    let token = email
        .token_for("/verify-email")
        .expect("verification email captured");
    get_c(app, &format!("/verify-email?token={token}"), None).await;
    let (_, _, cookie) = post_form(
        app,
        "/login",
        &[("email", addr), ("password", "password123")],
        None,
    )
    .await;
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
        assert!(
            matches!(s, StatusCode::SEE_OTHER | StatusCode::FOUND),
            "{uri} redirects anonymous: {s}"
        );
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
    let (s, _, _) = post_form(
        &app,
        "/parking/new",
        &[
            ("csrf", "bogus"),
            ("name", "X"),
            ("address", "Y"),
            ("parking_type", "rack"),
            ("lat", "-25.42"),
            ("lon", "-49.27"),
            ("timezone", "America/Sao_Paulo"),
        ],
        Some(&cookie),
    )
    .await;
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

    let (s, _, _) = post_form(
        &app,
        "/parking/new",
        &[
            ("csrf", &csrf),
            ("name", "Estação Centro Added"),
            ("address", "Rua X, 1"),
            ("parking_type", "rack"),
            ("cost_kind", "unknown"),
            ("lat", "-25.4284"),
            ("lon", "-49.2733"),
            ("timezone", "America/Sao_Paulo"),
            ("security", "well_lit"),
        ],
        Some(&cookie),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::SEE_OTHER,
        "add redirects to the new location: {s}"
    );

    // The location is persisted with creator attribution + version 1.
    let (id,): (i64,) = sqlx::query_as(
        "SELECT id FROM parking_location WHERE name = 'Estação Centro Added' ORDER BY id DESC LIMIT 1",
    ).fetch_one(&pool().await).await.unwrap();
    assert!(id > 0);
    let (version,): (i64,) = sqlx::query_as("SELECT version FROM parking_location WHERE id = $1")
        .bind(id)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    assert_eq!(version, 1);

    // The P3 details page renders.
    let (s, body) = get_c(&app, &format!("/parking/{id}"), Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("Estação Centro Added"));

    let _ = tx;
    cleanup_user_contributions(EMAIL).await;
}

#[db_test]
async fn favorite_toggle_and_list_work_for_authenticated_user(
    tx: &mut bikenest_test_support::TestTx,
) {
    const MARK: &str = "fix-http-fav";
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
    // Fixture location (committed) so the favorite repo can reference it.
    let loc = ParkingBuilder::new()
        .with_name("Favorite Target")
        .with_fixture_tag(MARK)
        .create(tx.executor())
        .await
        .unwrap();
    tx.commit_fixture().await;
    let loc_id = loc.id();

    let (app, _) = auth_app().await;
    let cookie = unverified_cookie(&app, "fav-user@example.com").await;

    // GET the details page to grab CSRF, then toggle favorite (auth-only).
    let (s, page) = get_c(&app, &format!("/parking/{loc_id}"), Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    let csrf = extract_csrf(&page);
    let (s, _, _) = post_form(
        &app,
        &format!("/parking/{loc_id}/favorite"),
        &[("csrf", &csrf)],
        Some(&cookie),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::OK,
        "favorite toggle succeeds for authenticated user"
    );

    // C4 lists the favorited location.
    let (s, body) = get_c(&app, "/account/favorites", Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        body.contains("Favorite Target"),
        "favorites page lists the spot"
    );

    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
    cleanup_user_contributions("fav-user@example.com").await;
}

// ---------------------------------------------------------------------------
// M3: added coverage (plan §9) — edit prefill/data-loss, revision in C5,
// pin-move→PENDING, review aggregate, rate-limit→429, identity absence.
// ---------------------------------------------------------------------------

/// POST /parking/new for a verified session; returns the created location id.
async fn add_location(
    app: &axum::Router,
    cookie: &str,
    csrf: &str,
    name: &str,
    extra: &[(&str, &str)],
) -> i64 {
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
    let refs: Vec<(&str, &str)> = fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let (s, _, _) = post_form(app, "/parking/new", &refs, Some(cookie)).await;
    assert_eq!(s, StatusCode::SEE_OTHER, "add should redirect: {s}");
    let (id,): (i64,) =
        sqlx::query_as("SELECT id FROM parking_location WHERE name = $1 ORDER BY id DESC LIMIT 1")
            .bind(name)
            .fetch_one(&pool().await)
            .await
            .unwrap();
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
    let id = add_location(
        &app,
        &cookie,
        &csrf,
        "Prefill Spot",
        &[
            ("cost_kind", "paid"),
            ("price", "1"),
            ("price_currency", "BRL"),
            ("price_unit", "hour"),
            ("security", "well_lit"),
            ("open_24h", "true"),
        ],
    )
    .await;

    // The edit page must pre-fill those values.
    let (s, edit_html) = get_c(&app, &format!("/parking/{id}/edit"), Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        edit_html.contains(r#"value="paid" selected"#),
        "cost pre-filled, not defaulted to free"
    );
    assert!(
        edit_html.contains("value=\"well_lit\""),
        "security pre-filled"
    );
    assert!(edit_html.contains("checked"), "open_24h pre-filled");

    // Editing only the name must NOT reset cost/security/hours.
    let edit_csrf = extract_csrf(&edit_html);
    let (s, _, _) = post_form(
        &app,
        &format!("/parking/{id}/edit"),
        &[
            ("csrf", &edit_csrf),
            ("version", "1"),
            ("name", "Prefill Spot Renamed"),
            ("address", "Rua X, 1"),
            ("parking_type", "rack"),
            ("cost_kind", "paid"),
            ("price", "1"),
            ("price_currency", "BRL"),
            ("price_unit", "hour"),
            ("security", "well_lit"),
            ("open_24h", "true"),
        ],
        Some(&cookie),
    )
    .await;
    assert_eq!(s, StatusCode::SEE_OTHER);

    let (name, cost_kind, price_cents, version): (String, String, Option<i64>, i64) =
        sqlx::query_as(
            "SELECT name, cost_kind, price_cents, version FROM parking_location WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool().await)
        .await
        .unwrap();
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
    let (s, _, _) = post_form(
        &app,
        &format!("/parking/{id}/edit"),
        &[
            ("csrf", &edit_csrf),
            ("version", "1"),
            ("name", "Revision Spot 2"),
            ("address", "Rua X, 1"),
            ("parking_type", "rack"),
            ("cost_kind", "unknown"),
            ("security", ""),
            ("open_24h", ""),
        ],
        Some(&cookie),
    )
    .await;
    assert_eq!(s, StatusCode::SEE_OTHER);

    // The revision row is recorded and C5 shows the edit.
    let (rev,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM parking_revision WHERE location_id = $1 AND change_kind = 'edit'",
    )
    .bind(id)
    .fetch_one(&pool().await)
    .await
    .unwrap();
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
    let (s, _, _) = post_form(
        &app,
        &format!("/parking/{id}/proposal"),
        &[
            ("csrf", &ecsrf),
            ("kind", "move_location"),
            ("lat", "-25.0"),
            ("lon", "-49.0"),
            ("timezone", "America/Sao_Paulo"),
            ("reason", "moved nearby"),
        ],
        Some(&cookie),
    )
    .await;
    assert_eq!(s, StatusCode::SEE_OTHER);

    // A PENDING proposal exists; the location is unchanged (still version 1).
    let (status,): (String,) = sqlx::query_as(
        "SELECT status FROM parking_proposal WHERE location_id = $1 AND kind = 'move_location'",
    )
    .bind(id)
    .fetch_one(&pool().await)
    .await
    .unwrap();
    assert_eq!(status, "PENDING");
    let (version,): (i64,) = sqlx::query_as("SELECT version FROM parking_location WHERE id = $1")
        .bind(id)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    assert_eq!(version, 1, "proposal causes no live change");

    // Following the redirect shows the "will be reviewed" confirmation.
    let (s, body) = get_c(&app, &format!("/parking/{id}?proposed=1"), Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        body.contains("will be reviewed by a moderator"),
        "proposal confirmation shown"
    );

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
    let rbody = multipart_review("4", "Great rack", "----bikenestphoto");
    let (s, _) = post_multipart(
        &app,
        &format!("/parking/{id}/review"),
        rbody,
        &cookie,
        Some(&rcsrf),
    )
    .await;
    assert_eq!(s, StatusCode::SEE_OTHER);

    let (count, avg): (i32, Option<f64>) = sqlx::query_as(
        "SELECT rating_count, rating_avg::float8 FROM parking_location WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&pool().await)
    .await
    .unwrap();
    assert_eq!(count, 1);
    assert!(
        (avg.unwrap() - 4.0).abs() < 0.001,
        "rating aggregate updated"
    );
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
        let (s, _, _) = post_form(
            &app,
            "/parking/new",
            &[
                ("csrf", &csrf),
                ("name", &name),
                ("address", "Rua X, 1"),
                ("parking_type", "rack"),
                ("cost_kind", "unknown"),
                ("lat", &lat.to_string()),
                ("lon", "-46.6"),
                ("timezone", "America/Sao_Paulo"),
            ],
            Some(&cookie),
        )
        .await;
        assert_eq!(s, StatusCode::SEE_OTHER, "add {i} should succeed");
    }
    let (s, _, _) = post_form(
        &app,
        "/parking/new",
        &[
            ("csrf", &csrf),
            ("name", "RateSpotFinal"),
            ("address", "Rua X, 1"),
            ("parking_type", "rack"),
            ("cost_kind", "unknown"),
            ("lat", "-23.4"),
            ("lon", "-46.6"),
            ("timezone", "America/Sao_Paulo"),
        ],
        Some(&cookie),
    )
    .await;
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
        .bind(EMAIL)
        .fetch_one(&pool().await)
        .await
        .unwrap();

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
        assert!(
            !body.contains(&format!(">{user_id}<")),
            "user id leaked on {uri}"
        );
    }

    let _ = tx;
    cleanup_user_contributions(EMAIL).await;
}

#[db_test]
async fn multiple_security_values_and_major_unit_price(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    const EMAIL: &str = "multi-sec@example.com";
    const MARK: &str = "fix-http-multisec";
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
    let cookie = verified_cookie(&app, &email, EMAIL).await;
    let (_, form) = get_c(&app, "/parking/new", Some(&cookie)).await;
    let csrf = extract_csrf(&form);

    // TWO security attributes (the source of the "duplicate field" crash) plus a
    // major-unit price ("1.50" must store as 150 cents, not raw text/cents).
    let id = add_location(
        &app,
        &cookie,
        &csrf,
        "Multi Security Spot",
        &[
            ("cost_kind", "paid"),
            ("price", "1.50"),
            ("price_currency", "BRL"),
            ("price_unit", "hour"),
            ("security", "well_lit,cctv"),
        ],
    )
    .await;

    let (cost_kind, price_cents, version): (String, Option<i64>, i64) = sqlx::query_as(
        "SELECT cost_kind, price_cents, version FROM parking_location WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&pool().await)
    .await
    .unwrap();
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
    let (s, _, _) = post_form(
        &app,
        &format!("/parking/{id}/edit"),
        &[
            ("csrf", &ecsrf),
            ("version", "1"),
            ("name", "Multi Security Spot"),
            ("address", "Rua X, 1"),
            ("parking_type", "rack"),
            ("cost_kind", "paid"),
            ("price", "1.50"),
            ("price_currency", "BRL"),
            ("price_unit", "hour"),
            ("security", "well_lit,cctv"),
            ("open_24h", ""),
        ],
        Some(&cookie),
    )
    .await;
    assert_eq!(s, StatusCode::SEE_OTHER);
    let (yes,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM parking_security WHERE location_id = $1 AND state = 1 AND feature_code IN ('well_lit','cctv')")
        .bind(id).fetch_one(&pool().await).await.unwrap();
    assert_eq!(yes, 2, "security preserved through edit");

    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
    cleanup_user_contributions(EMAIL).await;
}

// ---------------------------------------------------------------------------
// M4 photos — upload → queue → moderate → publish (§30/§44/§80)
// ---------------------------------------------------------------------------

fn tiny_jpeg() -> Vec<u8> {
    let img = image::RgbImage::from_pixel(32, 32, image::Rgb([20, 40, 60]));
    let mut b = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut b)
        .encode_image(&img)
        .unwrap();
    b
}

/// Build a multipart body with a `photo` file field and optional `alt` text.
fn multipart_upload(jpeg: &[u8], alt: Option<&str>, boundary: &str) -> (String, Vec<u8>) {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"photo\"; filename=\"photo.jpg\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: image/jpeg\r\n\r\n");
    body.extend_from_slice(jpeg);
    body.extend_from_slice(b"\r\n");
    if let Some(alt) = alt {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(b"Content-Disposition: form-data; name=\"alt\"\r\n\r\n");
        body.extend_from_slice(alt.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

/// Build a multipart review body (rating + body fields) using the test boundary.
fn multipart_review(rating: &str, body: &str, boundary: &str) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    b.extend_from_slice(b"Content-Disposition: form-data; name=\"rating\"\r\n\r\n");
    b.extend_from_slice(rating.as_bytes());
    b.extend_from_slice(b"\r\n");
    b.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    b.extend_from_slice(b"Content-Disposition: form-data; name=\"body\"\r\n\r\n");
    b.extend_from_slice(body.as_bytes());
    b.extend_from_slice(b"\r\n");
    b.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    b
}

async fn post_multipart(
    app: &axum::Router,
    uri: &str,
    body: Vec<u8>,
    cookie: &str,
    csrf: Option<&str>,
) -> (StatusCode, String) {
    let mut b = Request::builder()
        .method("POST")
        .uri(uri)
        .header("accept", "text/html")
        .header("Accept-Language", "en")
        .header(
            "content-type",
            "multipart/form-data; boundary=----bikenestphoto",
        )
        .header("cookie", cookie);
    if let Some(c) = csrf {
        b = b.header("X-CSRF-Token", c);
    }
    let res = app
        .clone()
        .oneshot(b.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).to_string())
}

async fn get_raw(app: &axum::Router, uri: &str) -> (StatusCode, Vec<u8>) {
    let res = app
        .clone()
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
    (status, body.to_vec())
}

fn has_exif_marker(bytes: &[u8]) -> bool {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return false;
    }
    let mut i = 2;
    while i + 1 < bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        if marker == 0xDA {
            break;
        }
        if i + 3 >= bytes.len() {
            break;
        }
        let len = (u16::from(bytes[i + 2]) << 8) | u16::from(bytes[i + 3]);
        if marker == 0xE1 {
            return true;
        }
        i += 2 + len as usize;
    }
    false
}

/// The media root used by `LocalDiskStorage::from_env` in tests.
fn media_root() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../media")
}

/// A committed fixture location (no photos) for photo tests.
async fn fixture_location(tx: &mut bikenest_test_support::TestTx, mark: &str, name: &str) -> i64 {
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(mark)
        .execute(&pool().await)
        .await
        .unwrap();
    let loc = ParkingBuilder::new()
        .with_name(name)
        .with_fixture_tag(mark)
        .create(tx.executor())
        .await
        .unwrap();
    tx.commit_fixture().await;
    loc.id()
}

async fn moderator_cookie(
    app: &axum::Router,
    email: &bikenest_infrastructure::FakeEmailProvider,
    addr: &str,
) -> String {
    cleanup_user_contributions(addr).await;
    post_form(
        app,
        "/register",
        &[("email", addr), ("password", "password123")],
        None,
    )
    .await;
    let token = email
        .token_for("/verify-email")
        .expect("moderator verification email");
    get_c(app, &format!("/verify-email?token={token}"), None).await;
    let (uid,): (i64,) = sqlx::query_as("SELECT id FROM users WHERE email = $1")
        .bind(addr)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO user_roles (user_id, role, granted_by) VALUES ($1, 'MODERATOR', NULL) ON CONFLICT DO NOTHING",
    )
    .bind(uid).execute(&pool().await).await.unwrap();
    let (_, _, cookie) = post_form(
        app,
        "/login",
        &[("email", addr), ("password", "password123")],
        None,
    )
    .await;
    cookie.unwrap().split(';').next().unwrap().to_string()
}

#[db_test]
async fn photo_upload_requires_verified(tx: &mut bikenest_test_support::TestTx) {
    let (app, _) = auth_app().await;
    let loc = fixture_location(tx, "photo-verify-gate", "Photo Verify Gate").await;
    let cookie = unverified_cookie(&app, "photo-unverified@example.com").await;
    let (s, page) = get_c(&app, &format!("/parking/{loc}"), Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    let csrf = extract_csrf(&page);

    let (ctype, body) = multipart_upload(&tiny_jpeg(), None, "----bikenestphoto");
    let _ = ctype;
    let (s, _) = post_multipart(
        &app,
        &format!("/parking/{loc}/photo"),
        body,
        &cookie,
        Some(&csrf),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "unverified upload blocked");
    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = 'photo-verify-gate'")
        .execute(&pool().await)
        .await
        .unwrap();
    cleanup_user_contributions("photo-unverified@example.com").await;
}

#[db_test]
async fn photo_upload_missing_csrf_header_is_forbidden(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    let loc = fixture_location(tx, "photo-csrf-gate", "Photo Csrf Gate").await;
    let cookie = verified_cookie(&app, &email, "photo-csrf@example.com").await;
    // No X-CSRF-Token header on a multipart POST → the middleware cannot read a
    // body csrf field and rejects with 403.
    let (_, body) = multipart_upload(&tiny_jpeg(), None, "----bikenestphoto");
    let (s, _) = post_multipart(&app, &format!("/parking/{loc}/photo"), body, &cookie, None).await;
    assert_eq!(
        s,
        StatusCode::FORBIDDEN,
        "multipart without CSRF header denied"
    );
    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = 'photo-csrf-gate'")
        .execute(&pool().await)
        .await
        .unwrap();
    cleanup_user_contributions("photo-csrf@example.com").await;
}

#[db_test]
async fn verified_upload_enters_queue_not_gallery(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    let loc = fixture_location(tx, "photo-queue-gate", "Photo Queue Gate").await;
    let cookie = verified_cookie(&app, &email, "photo-uploader@example.com").await;
    let (s, page) = get_c(&app, &format!("/parking/{loc}"), Some(&cookie)).await;
    assert_eq!(s, StatusCode::OK);
    let csrf = extract_csrf(&page);

    let (_, body) = multipart_upload(&tiny_jpeg(), Some("A first photo"), "----bikenestphoto");
    let (s, _) = post_multipart(
        &app,
        &format!("/parking/{loc}/photo"),
        body,
        &cookie,
        Some(&csrf),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "verified upload succeeds: {s}");

    // Row is PENDING_REVIEW with a thumbnail; not yet visible publicly.
    let (state, thumb): (String, Option<String>) = sqlx::query_as(
        "SELECT moderation_state, thumbnail_key FROM parking_photo WHERE location_id = $1",
    )
    .bind(loc)
    .fetch_one(&pool().await)
    .await
    .unwrap();
    assert_eq!(state, "PENDING_REVIEW");
    assert!(thumb.is_some(), "thumbnail derivative recorded");

    // The gallery (public) does not show a pending photo.
    let (s, gallery) = get_c(&app, &format!("/parking/{loc}"), None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        !gallery.contains("/media/uploads/"),
        "pending photo not in gallery"
    );
    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = 'photo-queue-gate'")
        .execute(&pool().await)
        .await
        .unwrap();
    cleanup_user_contributions("photo-uploader@example.com").await;
}

#[db_test]
async fn moderation_routes_require_moderator_role(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    // Anonymous is redirected to login (require_role → require_user → redirect).
    let (s, _) = get_c(&app, "/moderation/photos", None).await;
    assert!(
        matches!(s, StatusCode::SEE_OTHER | StatusCode::FOUND),
        "anonymous redirected to login: {s}"
    );

    // A verified non-moderator is blocked from the queue and from approving.
    let cookie = verified_cookie(&app, &email, "photo-nonmod@example.com").await;
    let (s, _) = get_c(&app, "/moderation/photos", Some(&cookie)).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "non-moderator cannot open queue");
    let (s, _, _) = post_form(
        &app,
        "/moderation/photos/parking/1/approve",
        &[("csrf", "bogus")],
        Some(&cookie),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "non-moderator cannot approve");
    let _ = tx;
    cleanup_user_contributions("photo-nonmod@example.com").await;
}

#[db_test]
async fn moderator_approve_publishes_to_gallery(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    let loc = fixture_location(tx, "photo-approve", "Photo Approve").await;
    let uploader = verified_cookie(&app, &email, "photo-approve-up@example.com").await;
    let (_, page) = get_c(&app, &format!("/parking/{loc}"), Some(&uploader)).await;
    let csrf = extract_csrf(&page);
    let (_, body) = multipart_upload(&tiny_jpeg(), None, "----bikenestphoto");
    post_multipart(
        &app,
        &format!("/parking/{loc}/photo"),
        body,
        &uploader,
        Some(&csrf),
    )
    .await;
    let (id,): (i64,) = sqlx::query_as("SELECT id FROM parking_photo WHERE location_id = $1")
        .bind(loc)
        .fetch_one(&pool().await)
        .await
        .unwrap();

    // The moderator opens the queue, grabs CSRF, and approves.
    let mod_cookie = moderator_cookie(&app, &email, "photo-moderator@example.com").await;
    let (s, queue) = get_c(&app, "/moderation/photos", Some(&mod_cookie)).await;
    assert_eq!(s, StatusCode::OK);
    let mcs = extract_csrf(&queue);
    let (s, _, _) = post_form(
        &app,
        &format!("/moderation/photos/parking/{id}/approve"),
        &[("csrf", &mcs)],
        Some(&mod_cookie),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "moderator approves: {s}");

    let (state,): (String,) =
        sqlx::query_as("SELECT moderation_state FROM parking_photo WHERE id = $1")
            .bind(id)
            .fetch_one(&pool().await)
            .await
            .unwrap();
    assert_eq!(state, "APPROVED");

    // Now the gallery serves the derivative.
    let (_, gallery) = get_c(&app, &format!("/parking/{loc}"), None).await;
    assert!(
        gallery.contains("/media/uploads/"),
        "approved photo appears in gallery"
    );
    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = 'photo-approve'")
        .execute(&pool().await)
        .await
        .unwrap();
    cleanup_user_contributions("photo-approve-up@example.com").await;
    cleanup_user_contributions("photo-moderator@example.com").await;
}

#[db_test]
async fn moderator_reject_deletes_object_and_hides(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    let loc = fixture_location(tx, "photo-reject", "Photo Reject").await;
    let uploader = verified_cookie(&app, &email, "photo-reject-up@example.com").await;
    let (_, page) = get_c(&app, &format!("/parking/{loc}"), Some(&uploader)).await;
    let csrf = extract_csrf(&page);
    let (_, body) = multipart_upload(&tiny_jpeg(), None, "----bikenestphoto");
    post_multipart(
        &app,
        &format!("/parking/{loc}/photo"),
        body,
        &uploader,
        Some(&csrf),
    )
    .await;
    let (id,): (i64,) = sqlx::query_as("SELECT id FROM parking_photo WHERE location_id = $1")
        .bind(loc)
        .fetch_one(&pool().await)
        .await
        .unwrap();

    let mod_cookie = moderator_cookie(&app, &email, "photo-reject-mod@example.com").await;
    let (_, queue) = get_c(&app, "/moderation/photos", Some(&mod_cookie)).await;
    let mcs = extract_csrf(&queue);
    let (s, _, _) = post_form(
        &app,
        &format!("/moderation/photos/parking/{id}/reject"),
        &[("csrf", &mcs), ("reason", "unclear image")],
        Some(&mod_cookie),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "moderator rejects: {s}");

    let (state, reason): (String, Option<String>) = sqlx::query_as(
        "SELECT moderation_state, rejection_reason FROM parking_photo WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&pool().await)
    .await
    .unwrap();
    assert_eq!(state, "REJECTED");
    assert_eq!(reason.as_deref(), Some("unclear image"));

    // The stored derivatives were deleted from the object store.
    let full = format!("{}/uploads/{id}/full.jpg", media_root());
    let thumb = format!("{}/uploads/{id}/thumb.jpg", media_root());
    assert!(
        !std::path::Path::new(&full).exists(),
        "full derivative deleted"
    );
    assert!(!std::path::Path::new(&thumb).exists(), "thumbnail deleted");
    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = 'photo-reject'")
        .execute(&pool().await)
        .await
        .unwrap();
    cleanup_user_contributions("photo-reject-up@example.com").await;
    cleanup_user_contributions("photo-reject-mod@example.com").await;
}

#[db_test]
async fn moderation_queue_hides_uploader_identity(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    let loc = fixture_location(tx, "photo-ident", "Photo Identity").await;
    const EMAIL: &str = "photo-ident-up@example.com";
    let uploader = verified_cookie(&app, &email, EMAIL).await;
    let (_, page) = get_c(&app, &format!("/parking/{loc}"), Some(&uploader)).await;
    let csrf = extract_csrf(&page);
    let (_, body) = multipart_upload(&tiny_jpeg(), Some("ident photo"), "----bikenestphoto");
    post_multipart(
        &app,
        &format!("/parking/{loc}/photo"),
        body,
        &uploader,
        Some(&csrf),
    )
    .await;

    let mod_cookie = moderator_cookie(&app, &email, "photo-ident-mod@example.com").await;
    let (s, queue) = get_c(&app, "/moderation/photos", Some(&mod_cookie)).await;
    assert_eq!(s, StatusCode::OK);
    // "Contributor #id" is shown; the uploader's email / OAuth subject is not (§80).
    assert!(
        queue.contains("Contributor"),
        "queue anonymizes the uploader"
    );
    assert!(!queue.contains(EMAIL), "uploader email never rendered");
    assert!(!queue.contains("sub-oauth"), "OAuth subject never rendered");
    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = 'photo-ident'")
        .execute(&pool().await)
        .await
        .unwrap();
    cleanup_user_contributions(EMAIL).await;
    cleanup_user_contributions("photo-ident-mod@example.com").await;
}

#[db_test]
async fn served_derivative_has_no_exif(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    let loc = fixture_location(tx, "photo-exif", "Photo Exif").await;
    let uploader = verified_cookie(&app, &email, "photo-exif-up@example.com").await;
    let (_, page) = get_c(&app, &format!("/parking/{loc}"), Some(&uploader)).await;
    let csrf = extract_csrf(&page);
    let (_, body) = multipart_upload(&tiny_jpeg(), None, "----bikenestphoto");
    post_multipart(
        &app,
        &format!("/parking/{loc}/photo"),
        body,
        &uploader,
        Some(&csrf),
    )
    .await;
    let (id,): (i64,) = sqlx::query_as("SELECT id FROM parking_photo WHERE location_id = $1")
        .bind(loc)
        .fetch_one(&pool().await)
        .await
        .unwrap();

    // Approve so it is served.
    let mod_cookie = moderator_cookie(&app, &email, "photo-exif-mod@example.com").await;
    let (_, queue) = get_c(&app, "/moderation/photos", Some(&mod_cookie)).await;
    let mcs = extract_csrf(&queue);
    post_form(
        &app,
        &format!("/moderation/photos/parking/{id}/approve"),
        &[("csrf", &mcs)],
        Some(&mod_cookie),
    )
    .await;

    // Fetch the presigned derivative (from the gallery <img src>) and confirm it
    // carries no EXIF/APP1.
    let (_, gallery) = get_c(&app, &format!("/parking/{loc}"), None).await;
    let anchor = r#"src="/media/uploads/"#;
    let start = gallery.find(anchor).expect("media thumb URL present");
    let after = &gallery[start + anchor.len()..];
    // Askama escapes `&` to `&amp;` in the src attribute; restore it so the
    // query params reach the media route intact.
    let url = format!(
        "/media/uploads/{}",
        &after[..after.find('"').expect("closing quote")]
    )
    .replace("&#38;", "&")
    .replace("&amp;", "&");
    let (status, bytes) = get_raw(&app, &url).await;
    assert_eq!(status, StatusCode::OK, "presigned derivative served");
    assert!(
        !has_exif_marker(&bytes),
        "served derivative has no EXIF/APP1"
    );
    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = 'photo-exif'")
        .execute(&pool().await)
        .await
        .unwrap();
    cleanup_user_contributions("photo-exif-up@example.com").await;
    cleanup_user_contributions("photo-exif-mod@example.com").await;
}

#[db_test]
async fn photo_upload_is_rate_limited(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    let loc = fixture_location(tx, "photo-rl", "Photo Rate Limit").await;
    let cookie = verified_cookie(&app, &email, "photo-rl-up@example.com").await;
    let (_, page) = get_c(&app, &format!("/parking/{loc}"), Some(&cookie)).await;
    let csrf = extract_csrf(&page);
    let jpeg = tiny_jpeg();
    for _ in 0..10 {
        let (_, body) = multipart_upload(&jpeg, None, "----bikenestphoto");
        let (s, _) = post_multipart(
            &app,
            &format!("/parking/{loc}/photo"),
            body,
            &cookie,
            Some(&csrf),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "day-1 upload allowed: {s}");
    }
    let (_, body) = multipart_upload(&jpeg, None, "----bikenestphoto");
    let (s, _) = post_multipart(
        &app,
        &format!("/parking/{loc}/photo"),
        body,
        &cookie,
        Some(&csrf),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::TOO_MANY_REQUESTS,
        "11th same-day upload rate-limited"
    );
    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = 'photo-rl'")
        .execute(&pool().await)
        .await
        .unwrap();
    cleanup_user_contributions("photo-rl-up@example.com").await;
}

async fn admin_cookie(
    app: &axum::Router,
    email: &bikenest_infrastructure::FakeEmailProvider,
    addr: &str,
) -> String {
    cleanup_user_contributions(addr).await;
    post_form(
        app,
        "/register",
        &[("email", addr), ("password", "password123")],
        None,
    )
    .await;
    let token = email
        .token_for("/verify-email")
        .expect("admin verification email");
    get_c(app, &format!("/verify-email?token={token}"), None).await;
    let (uid,): (i64,) = sqlx::query_as("SELECT id FROM users WHERE email = $1")
        .bind(addr)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO user_roles (user_id, role, granted_by) VALUES ($1, 'ADMIN', NULL) ON CONFLICT DO NOTHING",
    )
    .bind(uid).execute(&pool().await).await.unwrap();
    let (_, _, cookie) = post_form(
        app,
        "/login",
        &[("email", addr), ("password", "password123")],
        None,
    )
    .await;
    cookie.unwrap().split(';').next().unwrap().to_string()
}

#[db_test]
async fn admin_can_access_moderation_queue(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    let admin = admin_cookie(&app, &email, "photo-admin@example.com").await;
    let (s, body) = get_c(&app, "/moderation/photos", Some(&admin)).await;
    assert_eq!(
        s,
        StatusCode::OK,
        "admin (without Moderator role) can open the queue"
    );
    assert!(body.contains("Photo moderation"), "queue page renders");
    let _ = tx;
    cleanup_user_contributions("photo-admin@example.com").await;
}

#[db_test]
async fn photo_upload_alt_too_long_is_bad_request(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    let loc = fixture_location(tx, "photo-alt-long", "Photo Alt Long").await;
    let cookie = verified_cookie(&app, &email, "photo-alt-long-up@example.com").await;
    let (_, page) = get_c(&app, &format!("/parking/{loc}"), Some(&cookie)).await;
    let csrf = extract_csrf(&page);
    let long_alt = "a".repeat(501);
    let (_, body) = multipart_upload(&tiny_jpeg(), Some(&long_alt), "----bikenestphoto");
    let (s, _) = post_multipart(
        &app,
        &format!("/parking/{loc}/photo"),
        body,
        &cookie,
        Some(&csrf),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::BAD_REQUEST,
        "over-long caption rejected as 400"
    );
    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = 'photo-alt-long'")
        .execute(&pool().await)
        .await
        .unwrap();
    cleanup_user_contributions("photo-alt-long-up@example.com").await;
}

// ---------------------------------------------------------------------------
// M5 moderation & reporting — end-to-end (report → claim → resolve → hide;
// invalidate/restore parking; suspend/restore; audit viewer gating; the
// self-resolve guard; D3 multipart review-photo attach).
// ---------------------------------------------------------------------------

async fn last_report_id(reporter_email: &str, target_type: &str, target_id: i64) -> i64 {
    let (id,): (i64,) = sqlx::query_as(
        "SELECT id FROM report WHERE reporter_id = (SELECT id FROM users WHERE email = $1) \
         AND target_type = $2 AND target_id = $3 ORDER BY id DESC LIMIT 1",
    )
    .bind(reporter_email)
    .bind(target_type)
    .bind(target_id)
    .fetch_one(&pool().await)
    .await
    .unwrap();
    id
}

#[db_test]
async fn report_review_flow_claim_resolve_hides_and_audits(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    const UPLOADER: &str = "m5-up@example.com";
    const REPORTER: &str = "m5-reporter@example.com";
    const MOD: &str = "m5-mod@example.com";
    let loc = fixture_location(tx, "m5-report-loc", "M5 Report Loc").await;

    // Uploader (verified) writes a review (D3 multipart).
    let uploader = verified_cookie(&app, &email, UPLOADER).await;
    let (_, rev_form) = get_c(&app, &format!("/parking/{loc}/review"), Some(&uploader)).await;
    let rcsrf = extract_csrf(&rev_form);
    let rbody = multipart_review("5", "Great secured rack", "----bikenestphoto");
    let (s, _) = post_multipart(
        &app,
        &format!("/parking/{loc}/review"),
        rbody,
        &uploader,
        Some(&rcsrf),
    )
    .await;
    assert_eq!(s, StatusCode::SEE_OTHER, "review created: {s}");
    let (review_id,): (i64,) =
        sqlx::query_as("SELECT id FROM review WHERE location_id = $1 ORDER BY id DESC LIMIT 1")
            .bind(loc)
            .fetch_one(&pool().await)
            .await
            .unwrap();

    // Reporter (authenticated, brand-new — not verified) reports the review.
    let reporter = unverified_cookie(&app, REPORTER).await;
    let (_, rep_page) = get_c(&app, &format!("/parking/{loc}"), Some(&reporter)).await;
    let csrf = extract_csrf(&rep_page);
    let (s, _, _) = post_form(
        &app,
        "/reports",
        &[
            ("csrf", &csrf),
            ("target_type", "review"),
            ("target_id", &review_id.to_string()),
            ("reason", "spam"),
            ("description", "Looks fake"),
        ],
        Some(&reporter),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "report submitted: {s}");
    let report_id = last_report_id(REPORTER, "review", review_id).await;

    let (state,): (String,) = sqlx::query_as("SELECT state FROM report WHERE id = $1")
        .bind(report_id)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    assert_eq!(state, "OPEN");
    let (created_audits,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM audit_events WHERE action = 'report.created' AND target_id = $1",
    )
    .bind(report_id.to_string())
    .fetch_one(&pool().await)
    .await
    .unwrap();
    assert_eq!(created_audits, 1);

    // Moderator claims then resolves (hides the review).
    let mod_cookie = moderator_cookie(&app, &email, MOD).await;
    let (_, mod_page) = get_c(&app, "/moderation/reports?state=OPEN", Some(&mod_cookie)).await;
    let mcsrf = extract_csrf(&mod_page);
    let (s, _, _) = post_form(
        &app,
        &format!("/moderation/reports/{report_id}/claim"),
        &[("csrf", &mcsrf)],
        Some(&mod_cookie),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "claim: {s}");
    let (s, _, _) = post_form(
        &app,
        &format!("/moderation/reports/{report_id}/resolve"),
        &[("csrf", &mcsrf), ("note", "spam review")],
        Some(&mod_cookie),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "resolve: {s}");

    let (state,): (String,) = sqlx::query_as("SELECT state FROM report WHERE id = $1")
        .bind(report_id)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    assert_eq!(state, "RESOLVED");
    let (rstate,): (String,) = sqlx::query_as("SELECT moderation_state FROM review WHERE id = $1")
        .bind(review_id)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    assert_eq!(rstate, "HIDDEN", "resolved report hides the review");

    // The hidden review disappears from public P3.
    let (_, pub_page) = get_c(&app, &format!("/parking/{loc}"), None).await;
    assert!(
        !pub_page.contains("Great secured rack"),
        "hidden review not rendered"
    );

    // Audit trail records who did what.
    let (claimed,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM audit_events WHERE action = 'report.claimed' AND target_id = $1",
    )
    .bind(report_id.to_string())
    .fetch_one(&pool().await)
    .await
    .unwrap();
    assert_eq!(claimed, 1);
    let (resolved,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM audit_events WHERE action = 'report.resolved' AND target_id = $1",
    )
    .bind(report_id.to_string())
    .fetch_one(&pool().await)
    .await
    .unwrap();
    assert_eq!(resolved, 1);

    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = 'm5-report-loc'")
        .execute(&pool().await)
        .await
        .unwrap();
    cleanup_user_contributions(UPLOADER).await;
    cleanup_user_contributions(REPORTER).await;
    cleanup_user_contributions(MOD).await;
}

#[db_test]
async fn moderator_cannot_resolve_own_report(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    const MOD: &str = "m5-selfmod@example.com";
    let loc = fixture_location(tx, "m5-self-loc", "M5 Self Loc").await;
    let mod_cookie = moderator_cookie(&app, &email, MOD).await;
    let (_, mod_page) = get_c(&app, &format!("/parking/{loc}"), Some(&mod_cookie)).await;
    let csrf = extract_csrf(&mod_page);

    // The moderator submits a report, then tries to resolve it themselves.
    let (s, _, _) = post_form(
        &app,
        "/reports",
        &[
            ("csrf", &csrf),
            ("target_type", "parking"),
            ("target_id", &loc.to_string()),
            ("reason", "spam"),
        ],
        Some(&mod_cookie),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let report_id = last_report_id(MOD, "parking", loc).await;

    // Claim → allowed, but self-resolve → CONFLICT/self-resolve error.
    let (_, mod_page2) = get_c(&app, "/moderation/reports?state=OPEN", Some(&mod_cookie)).await;
    let mcsrf = extract_csrf(&mod_page2);
    post_form(
        &app,
        &format!("/moderation/reports/{report_id}/claim"),
        &[("csrf", &mcsrf)],
        Some(&mod_cookie),
    )
    .await;
    let (s, body, _) = post_form(
        &app,
        &format!("/moderation/reports/{report_id}/resolve"),
        &[("csrf", &mcsrf), ("note", "x")],
        Some(&mod_cookie),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT, "self-resolve rejected: {s}");
    assert!(
        body.contains("cannot resolve"),
        "friendly self-resolve message: {body}"
    );

    let (state,): (String,) = sqlx::query_as("SELECT state FROM report WHERE id = $1")
        .bind(report_id)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    assert_eq!(
        state, "UNDER_REVIEW",
        "report stays under review after rejected self-resolve"
    );

    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = 'm5-self-loc'")
        .execute(&pool().await)
        .await
        .unwrap();
    cleanup_user_contributions(MOD).await;
}

#[db_test]
async fn invalidate_parking_public_404_moderator_banner_and_restore(
    tx: &mut bikenest_test_support::TestTx,
) {
    let (app, email) = auth_app().await;
    const MOD: &str = "m5-inv-mod@example.com";
    let loc = fixture_location(tx, "m5-inv-loc", "M5 Invalidate Loc").await;

    // Public sees the active listing.
    let (s, _) = get_c(&app, &format!("/parking/{loc}"), None).await;
    assert_eq!(s, StatusCode::OK);

    let mod_cookie = moderator_cookie(&app, &email, MOD).await;
    let (_, mod_page) = get_c(&app, "/moderation", Some(&mod_cookie)).await;
    let mcsrf = extract_csrf(&mod_page);
    let (s, _, _) = post_form(
        &app,
        &format!("/moderation/parking/{loc}/invalidate"),
        &[("csrf", &mcsrf)],
        Some(&mod_cookie),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "invalidate: {s}");

    // Public P3 now 404s; the moderator still sees it with a banner.
    let (s, _) = get_c(&app, &format!("/parking/{loc}"), None).await;
    assert_eq!(
        s,
        StatusCode::NOT_FOUND,
        "public cannot see invalidated parking"
    );
    let (s, body) = get_c(&app, &format!("/parking/{loc}"), Some(&mod_cookie)).await;
    assert_eq!(s, StatusCode::OK, "moderator still sees the page");
    assert!(
        body.contains("under moderation"),
        "moderator banner rendered"
    );

    // Also absent from search (search filters ACTIVE).
    let (_, search) = get_c(&app, "/search?lat=-25.4284&lon=-49.2733&radius=2000", None).await;
    assert!(!search.contains("M5 Invalidate Loc"));

    // Restore brings it back (grab a fresh CSRF from the dashboard).
    let (_, mod_page2) = get_c(&app, "/moderation", Some(&mod_cookie)).await;
    let mcsrf2 = extract_csrf(&mod_page2);
    let (s, _, _) = post_form(
        &app,
        &format!("/moderation/parking/{loc}/restore"),
        &[("csrf", &mcsrf2)],
        Some(&mod_cookie),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "restore: {s}");
    let (s, _) = get_c(&app, &format!("/parking/{loc}"), None).await;
    assert_eq!(s, StatusCode::OK, "public sees restored listing");

    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = 'm5-inv-loc'")
        .execute(&pool().await)
        .await
        .unwrap();
    cleanup_user_contributions(MOD).await;
}

#[db_test]
async fn admin_suspend_revokes_sessions_blocks_and_restore(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    const USER: &str = "m5-suspend@example.com";
    const ADMIN: &str = "m5-admin@example.com";
    // A verified user with a live session.
    let user_cookie = verified_cookie(&app, &email, USER).await;
    let admin_cookie = admin_cookie(&app, &email, ADMIN).await;
    let (_, admin_page) = get_c(&app, "/admin/users", Some(&admin_cookie)).await;
    let acsrf = extract_csrf(&admin_page);
    let (uid,): (i64,) = sqlx::query_as("SELECT id FROM users WHERE email = $1")
        .bind(USER)
        .fetch_one(&pool().await)
        .await
        .unwrap();

    // Suspend: session revoked + state SUSPENDED + audited.
    let (s, _, _) = post_form(
        &app,
        &format!("/admin/users/{uid}/suspend"),
        &[("csrf", &acsrf)],
        Some(&admin_cookie),
    )
    .await;
    assert_eq!(s, StatusCode::SEE_OTHER, "suspend redirects: {s}");
    let (state,): (String,) = sqlx::query_as("SELECT account_state FROM users WHERE id = $1")
        .bind(uid)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    assert_eq!(state, "SUSPENDED");
    let (revoked,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM sessions WHERE user_id = $1 AND revoked_at IS NOT NULL",
    )
    .bind(uid)
    .fetch_one(&pool().await)
    .await
    .unwrap();
    assert!(revoked >= 1, "suspension revokes sessions");
    let (audit,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM audit_events WHERE action = 'user.suspended' AND target_id = $1",
    )
    .bind(uid.to_string())
    .fetch_one(&pool().await)
    .await
    .unwrap();
    assert_eq!(audit, 1);

    // The suspended user's existing session is now dead → redirected to login.
    let (s, _) = get_c(&app, "/account", Some(&user_cookie)).await;
    assert!(
        matches!(s, StatusCode::SEE_OTHER | StatusCode::FOUND),
        "mid-session gate: {s}"
    );

    // Login is blocked with the generic message.
    let (_, body, _) = post_form(
        &app,
        "/login",
        &[("email", USER), ("password", "password123")],
        None,
    )
    .await;
    assert!(
        body.contains("Email or password is incorrect"),
        "suspended login blocked: {body}"
    );

    // Restore → ACTIVE + login works.
    let (_, admin_page2) = get_c(&app, "/admin/users", Some(&admin_cookie)).await;
    let acsrf2 = extract_csrf(&admin_page2);
    let (s, _, _) = post_form(
        &app,
        &format!("/admin/users/{uid}/restore"),
        &[("csrf", &acsrf2)],
        Some(&admin_cookie),
    )
    .await;
    assert_eq!(s, StatusCode::SEE_OTHER, "restore redirects: {s}");
    let (_, _, new_cookie) = post_form(
        &app,
        "/login",
        &[("email", USER), ("password", "password123")],
        None,
    )
    .await;
    assert!(
        new_cookie.as_deref().unwrap_or("").contains("session_id="),
        "restored user can log in"
    );

    let _ = tx;
    cleanup_user_contributions(USER).await;
    cleanup_user_contributions(ADMIN).await;
}

#[db_test]
async fn moderation_and_audit_routes_are_gated(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    // Anonymous → redirected to login.
    for uri in [
        "/moderation",
        "/moderation/reports",
        "/moderation/proposals",
    ] {
        let (s, _) = get_c(&app, uri, None).await;
        assert!(
            matches!(s, StatusCode::SEE_OTHER | StatusCode::FOUND),
            "{uri} anonymous: {s}"
        );
    }
    // Non-moderator verified user → 403 on every moderation route.
    let cookie = verified_cookie(&app, &email, "m5-nonmod@example.com").await;
    let (s, _) = get_c(&app, "/moderation", Some(&cookie)).await;
    assert_eq!(
        s,
        StatusCode::FORBIDDEN,
        "non-moderator cannot open dashboard"
    );
    let (s, _) = get_c(&app, "/admin/audit", Some(&cookie)).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "audit viewer is admin-only");
    // A moderator (not admin) cannot open the audit viewer.
    let mod_cookie = moderator_cookie(&app, &email, "m5-mod-gate@example.com").await;
    let (s, _) = get_c(&app, "/admin/audit", Some(&mod_cookie)).await;
    assert_eq!(
        s,
        StatusCode::FORBIDDEN,
        "moderator (not admin) cannot open audit viewer"
    );
    let _ = tx;
    cleanup_user_contributions("m5-nonmod@example.com").await;
    cleanup_user_contributions("m5-mod-gate@example.com").await;
}

#[db_test]
async fn d3_review_photos_held_pending_until_approved(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    const AUTHOR: &str = "m5-review-photo@example.com";
    let loc = fixture_location(tx, "m5-rp-loc", "M5 Review Photo Loc").await;
    let cookie = verified_cookie(&app, &email, AUTHOR).await;
    let (_, form) = get_c(&app, &format!("/parking/{loc}/review"), Some(&cookie)).await;
    let csrf = extract_csrf(&form);

    // D3 multipart review with an attached photo (rating + body + photo in one body).
    let jpeg = tiny_jpeg();
    let mut body = Vec::new();
    body.extend_from_slice(
        b"------bikenestphoto\r\nContent-Disposition: form-data; name=\"rating\"\r\n\r\n4\r\n",
    );
    body.extend_from_slice(b"------bikenestphoto\r\nContent-Disposition: form-data; name=\"body\"\r\n\r\nNice locker\r\n");
    body.extend_from_slice(b"------bikenestphoto\r\nContent-Disposition: form-data; name=\"photo\"; filename=\"r.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n");
    body.extend_from_slice(&jpeg);
    body.extend_from_slice(b"\r\n------bikenestphoto--\r\n");
    let (s, _) = post_multipart(
        &app,
        &format!("/parking/{loc}/review"),
        body,
        &cookie,
        Some(&csrf),
    )
    .await;
    assert_eq!(s, StatusCode::SEE_OTHER, "review with photo: {s}");

    let (review_id,): (i64,) =
        sqlx::query_as("SELECT id FROM review WHERE location_id = $1 ORDER BY id DESC LIMIT 1")
            .bind(loc)
            .fetch_one(&pool().await)
            .await
            .unwrap();
    let (rstate,): (String,) = sqlx::query_as("SELECT moderation_state FROM review WHERE id = $1")
        .bind(review_id)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    assert_eq!(rstate, "ACTIVE", "review text publishes immediately");

    // The review photo is held PENDING_REVIEW; only approved render.
    let (pstate,): (String,) =
        sqlx::query_as("SELECT moderation_state FROM review_photo WHERE review_id = $1")
            .bind(review_id)
            .fetch_one(&pool().await)
            .await
            .unwrap();
    assert_eq!(pstate, "PENDING_REVIEW", "review photo held pending");

    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = 'm5-rp-loc'")
        .execute(&pool().await)
        .await
        .unwrap();
    cleanup_user_contributions(AUTHOR).await;
}

#[db_test]
async fn approved_review_photo_renders_on_p3(tx: &mut bikenest_test_support::TestTx) {
    let (app, email) = auth_app().await;
    const AUTHOR: &str = "m5-rp-render@example.com";
    const MOD: &str = "m5-rp-mod@example.com";
    let loc = fixture_location(tx, "m5-rp-render-loc", "M5 Review Render Loc").await;
    let cookie = verified_cookie(&app, &email, AUTHOR).await;
    let (_, form) = get_c(&app, &format!("/parking/{loc}/review"), Some(&cookie)).await;
    let csrf = extract_csrf(&form);

    // D3 multipart review with an attached (pending) photo.
    let jpeg = tiny_jpeg();
    let mut body = Vec::new();
    body.extend_from_slice(
        b"------bikenestphoto\r\nContent-Disposition: form-data; name=\"rating\"\r\n\r\n5\r\n",
    );
    body.extend_from_slice(b"------bikenestphoto\r\nContent-Disposition: form-data; name=\"body\"\r\n\r\nGreat locker\r\n");
    body.extend_from_slice(b"------bikenestphoto\r\nContent-Disposition: form-data; name=\"photo\"; filename=\"r.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n");
    body.extend_from_slice(&jpeg);
    body.extend_from_slice(b"\r\n------bikenestphoto--\r\n");
    let (s, _) = post_multipart(
        &app,
        &format!("/parking/{loc}/review"),
        body,
        &cookie,
        Some(&csrf),
    )
    .await;
    assert_eq!(s, StatusCode::SEE_OTHER);

    let (review_id,): (i64,) =
        sqlx::query_as("SELECT id FROM review WHERE location_id = $1 ORDER BY id DESC LIMIT 1")
            .bind(loc)
            .fetch_one(&pool().await)
            .await
            .unwrap();
    let (rp_id,): (i64,) = sqlx::query_as("SELECT id FROM review_photo WHERE review_id = $1")
        .bind(review_id)
        .fetch_one(&pool().await)
        .await
        .unwrap();

    // Pending → not yet rendered on the public P3.
    let (_, pub_page) = get_c(&app, &format!("/parking/{loc}"), None).await;
    assert!(
        !pub_page.contains("/media/uploads/"),
        "pending review photo not rendered"
    );

    // Moderator approves it from the unified queue (kind=review).
    let mod_cookie = moderator_cookie(&app, &email, MOD).await;
    let (_, mod_page) = get_c(&app, "/moderation/photos", Some(&mod_cookie)).await;
    let mcsrf = extract_csrf(&mod_page);
    let (s, _, _) = post_form(
        &app,
        &format!("/moderation/photos/review/{rp_id}/approve"),
        &[("csrf", &mcsrf)],
        Some(&mod_cookie),
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    // Now the approved review photo renders on P3.
    let (_, pub_page) = get_c(&app, &format!("/parking/{loc}"), None).await;
    assert!(
        pub_page.contains("/media/uploads/"),
        "approved review photo renders on P3"
    );

    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = 'm5-rp-render-loc'")
        .execute(&pool().await)
        .await
        .unwrap();
    cleanup_user_contributions(AUTHOR).await;
    cleanup_user_contributions(MOD).await;
}

// ---------------------------------------------------------------------------
// Template hygiene (pure filesystem scan, no DB)
// ---------------------------------------------------------------------------

/// `--color-error` was renamed to `--color-danger` in input.css; any
/// `text-error`/`bg-error`/`border-error`/`hover:border-error` utility left in
/// a template renders as dead CSS (no matching Tailwind class exists).
#[test]
fn no_error_colour_classes_remain_in_templates() {
    let templates_dir =
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../templates"));
    let pattern = regex::Regex::new(r"\b(text|bg|border|hover:border)-error\b").unwrap();
    let mut offenders = Vec::new();
    let mut stack = vec![templates_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in
            std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(contents) = std::fs::read_to_string(&path) {
                for m in pattern.find_iter(&contents) {
                    offenders.push(format!("{}: {}", path.display(), m.as_str()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "found dead `-error` colour classes:\n{}",
        offenders.join("\n")
    );
}
