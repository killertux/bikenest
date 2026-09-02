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
