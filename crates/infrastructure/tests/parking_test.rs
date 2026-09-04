//! Real-PostgreSQL integration tests for the parking readers (§31–§32).
//!
//! The readers run on pool connections, which cannot see rows inside this
//! test's transaction — so these tests use the **committed-fixture pattern**:
//! rows are created via `ParkingBuilder` with a unique `seed_key` fixture tag,
//! `tx.commit_fixture()` commits them, assertions run against the real
//! readers, and the tag is deleted (at start and end) via the shared pool.
//! Everything else in the suite still uses transaction-per-test rollback.

use bikenest_application::{
    CostFilter, Cursor, ParkingDetailsReader, ReaderError, SearchInput, SearchPage, SearchParking,
    SearchRequest, Sort,
};
use bikenest_domain::{Cost, CurrencyCode, Money, ParkingType, PricingUnit};
use bikenest_test_support::{ParkingBuilder, db_test, pool};

/// Base origin: far from any seed data (Serra da Cantareira area).
const ORIGIN: (f64, f64) = (-23.400_000, -46.600_000);

/// Each test gets its own geographic patch (~5.5 km apart) so leftover
/// fixture rows from a crashed run of another test never interfere:
/// the largest test radius (1 km) cannot reach a neighboring patch.
fn test_origin(k: f64) -> bikenest_domain::GeoPoint {
    bikenest_domain::GeoPoint::new(ORIGIN.0 + k * 0.05, ORIGIN.1).unwrap()
}

/// Place a builder `meters` north of test patch `k`'s origin.
fn at(k: f64, meters: f64) -> ParkingBuilder {
    let o = test_origin(k);
    ParkingBuilder::new().at(o.lat() + meters / 111_320.0, o.lon())
}

fn request_at(k: f64, sort: Sort) -> SearchRequest {
    SearchRequest::new(
        test_origin(k),
        Some("test origin".to_string()),
        1000,
        Default::default(),
        sort,
        20,
        None,
    )
}

/// Deletes committed fixture rows for a test tag (via the shared pool).
async fn cleanup_fixture(marker: &str) {
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(marker)
        .execute(&pool().await)
        .await
        .expect("cleanup fixture");
}

/// A fixed instant (noon in São Paulo — clear of every fixture's hour
/// boundaries in this file) so `real_search` results never depend on the
/// wall-clock time the suite happens to run at. Tests that specifically
/// exercise "open now" behavior use `search_at`/`sql_open_now` with their own
/// pinned instants instead.
fn fixed_now() -> chrono::DateTime<chrono::Utc> {
    instant_at("America/Sao_Paulo", 2026, 3, 10, 12, 0)
}

async fn real_search(
    request: &SearchRequest,
    limit: usize,
    apply_cursor: bool,
) -> Result<SearchPage, ReaderError> {
    let db = bikenest_infrastructure::Db::from_pool(pool().await);
    bikenest_infrastructure::SqlxParkingSearchReader::new(db)
        .search_at(request, limit, apply_cursor, fixed_now())
        .await
}

async fn real_details(id: i64) -> Result<Option<bikenest_domain::ParkingLocation>, ReaderError> {
    let db = bikenest_infrastructure::Db::from_pool(pool().await);
    bikenest_infrastructure::SqlxParkingDetailsReader::new(db)
        .details(id)
        .await
}

#[db_test]
async fn within_radius_ordered_by_distance_with_correct_total(tx: &mut TestTx) {
    const MARK: &str = "fix-within-radius";
    cleanup_fixture(MARK).await;
    let conn = tx.executor();
    at(1.0, 100.0)
        .with_fixture_tag(MARK)
        .with_name("A 100m")
        .create(&mut *conn)
        .await
        .unwrap();
    at(1.0, 300.0)
        .with_fixture_tag(MARK)
        .with_name("B 300m")
        .create(&mut *conn)
        .await
        .unwrap();
    at(1.0, 2000.0)
        .with_fixture_tag(MARK)
        .with_name("C 2km")
        .create(&mut *conn)
        .await
        .unwrap();
    tx.commit_fixture().await;

    let mut req = request_at(1.0, Sort::Distance);
    req.radius_m = 500;
    let page = real_search(&req, 20, false).await.unwrap();
    assert_eq!(page.items.len(), 2, "2km location is outside the radius");
    assert_eq!(page.total, 2);
    assert_eq!(page.items[0].name, "A 100m");
    assert_eq!(page.items[1].name, "B 300m");
    assert!(page.items[0].distance_m < page.items[1].distance_m);
    cleanup_fixture(MARK).await;
}

#[db_test]
async fn radius_filter_excludes_far_locations(tx: &mut TestTx) {
    const MARK: &str = "fix-radius";
    cleanup_fixture(MARK).await;
    let conn = tx.executor();
    at(2.0, 50.0)
        .with_fixture_tag(MARK)
        .create(&mut *conn)
        .await
        .unwrap();
    at(2.0, 1500.0)
        .with_fixture_tag(MARK)
        .create(&mut *conn)
        .await
        .unwrap();
    tx.commit_fixture().await;

    let mut req = request_at(2.0, Sort::Distance);
    req.radius_m = 250;
    let page = real_search(&req, 20, false).await.unwrap();
    assert_eq!(page.total, 1);
    assert!(page.items[0].distance_m < 60.0);
    cleanup_fixture(MARK).await;
}

#[db_test]
async fn cost_type_and_security_filters_apply(tx: &mut TestTx) {
    const MARK: &str = "fix-filters";
    cleanup_fixture(MARK).await;
    let conn = tx.executor();
    at(3.0, 30.0)
        .with_fixture_tag(MARK)
        .with_type(ParkingType::Rack)
        .with_security("cctv", 1)
        .create(&mut *conn)
        .await
        .unwrap();
    at(3.0, 60.0)
        .with_fixture_tag(MARK)
        .with_cost(Cost::Paid {
            price: Some(Money::new(
                500,
                CurrencyCode::parse("BRL").unwrap(),
                PricingUnit::Day,
            )),
        })
        .with_type(ParkingType::Locker)
        .create(&mut *conn)
        .await
        .unwrap();
    at(3.0, 90.0)
        .with_fixture_tag(MARK)
        .with_type(ParkingType::Rack)
        .with_cost(Cost::Unknown)
        .create(&mut *conn)
        .await
        .unwrap();
    tx.commit_fixture().await;

    let mut req = request_at(3.0, Sort::Distance);
    req.radius_m = 250;

    // Cost filter: free only.
    req.filters.cost = Some(CostFilter::Free);
    let page = real_search(&req, 20, false).await.unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].cost.kind_code(), "free");

    // Type filter: rack only (free + unknown-cost racks).
    req.filters.cost = None;
    req.filters.types = vec![ParkingType::Rack];
    let page = real_search(&req, 20, false).await.unwrap();
    assert_eq!(page.total, 2);

    // Security all-of: cctv=yes required.
    req.filters.types.clear();
    req.filters.security_all = vec!["cctv".to_string()];
    let page = real_search(&req, 20, false).await.unwrap();
    assert_eq!(page.total, 1);
    assert!(page.items[0].security_yes.contains(&"cctv".to_string()));

    // Security all-of with two required features → none match.
    req.filters.security_all = vec!["cctv".to_string(), "security_guard".to_string()];
    let page = real_search(&req, 20, false).await.unwrap();
    assert_eq!(page.total, 0);
    cleanup_fixture(MARK).await;
}

#[db_test]
async fn open_now_filter_agrees_with_domain_for_all_day_hours(tx: &mut TestTx) {
    const MARK: &str = "fix-open-now";
    cleanup_fixture(MARK).await;
    let conn = tx.executor();
    let all_day = at(4.0, 30.0)
        .with_fixture_tag(MARK)
        .with_all_day_hours(1..=7)
        .create(&mut *conn)
        .await
        .unwrap();
    let narrow = at(4.0, 60.0)
        .with_fixture_tag(MARK)
        .with_hours(1..=7, (3, 0), (4, 0)) // open only 03:00–04:00 local
        .create(&mut *conn)
        .await
        .unwrap();
    tx.commit_fixture().await;

    // Pinned to noon in São Paulo (Tue 2026-03-10) — clear of the narrow
    // 03:00-04:00 window regardless of when the suite itself runs, and
    // checked against the domain rule directly (not just the SQL flag).
    let now = instant_at("America/Sao_Paulo", 2026, 3, 10, 12, 0);
    assert!(
        domain_open_now(&all_day, now),
        "domain: all-day location is open at noon"
    );
    assert!(
        !domain_open_now(&narrow, now),
        "domain: narrow 03:00-04:00 window is closed at noon"
    );

    let mut req = request_at(4.0, Sort::Distance);
    req.filters.open_now = true;
    let page = search_at(&req, now).await.unwrap();
    assert_eq!(page.total, 1);
    assert!(
        page.items[0].is_open_now,
        "all-day location must be open now"
    );
    cleanup_fixture(MARK).await;
}

#[db_test]
async fn non_active_locations_are_hidden_from_search(tx: &mut TestTx) {
    const MARK: &str = "fix-moderation";
    cleanup_fixture(MARK).await;
    let conn = tx.executor();
    at(5.0, 30.0)
        .with_fixture_tag(MARK)
        .with_moderation_state("INVALID")
        .create(&mut *conn)
        .await
        .unwrap();
    at(5.0, 60.0)
        .with_fixture_tag(MARK)
        .create(&mut *conn)
        .await
        .unwrap();
    tx.commit_fixture().await;

    let page = real_search(&request_at(5.0, Sort::Distance), 20, false)
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    cleanup_fixture(MARK).await;
}

#[db_test]
async fn keyset_pagination_is_stable_across_inserts(tx: &mut TestTx) {
    const MARK: &str = "fix-keyset";
    cleanup_fixture(MARK).await;
    let conn = tx.executor();
    for m in 1..=25 {
        at(6.0, m as f64 * 10.0)
            .with_fixture_tag(MARK)
            .create(&mut *conn)
            .await
            .unwrap();
    }
    tx.commit_fixture().await;

    let mut req = request_at(6.0, Sort::Distance);
    let mut seen: Vec<i64> = Vec::new();
    for _ in 0..8 {
        let page = real_search(&req, 5, true).await.unwrap(); // page of 4 + lookahead
        for item in &page.items {
            assert!(!seen.contains(&item.id), "duplicate id across pages");
            seen.push(item.id);
        }
        if page.items.len() <= 4 {
            break;
        }
        let last = page.items.last().unwrap();
        req.cursor = Some(Cursor {
            sort: Sort::Distance,
            v: last.distance_m,
            id: last.id,
        });
    }
    assert_eq!(seen.len(), 25, "all 25 items paginated exactly once");
    cleanup_fixture(MARK).await;
}

#[db_test]
async fn rating_and_recently_verified_sorts_work(tx: &mut TestTx) {
    const MARK: &str = "fix-sorts";
    cleanup_fixture(MARK).await;
    let conn = tx.executor();
    at(7.0, 30.0)
        .with_fixture_tag(MARK)
        .with_rating(3.0, 2)
        .create(&mut *conn)
        .await
        .unwrap();
    at(7.0, 60.0)
        .with_fixture_tag(MARK)
        .with_rating(4.9, 10)
        .create(&mut *conn)
        .await
        .unwrap();
    at(7.0, 90.0)
        .with_fixture_tag(MARK)
        .never_verified()
        .create(&mut *conn)
        .await
        .unwrap();
    tx.commit_fixture().await;

    let page = real_search(&request_at(7.0, Sort::Rating), 20, false)
        .await
        .unwrap();
    assert!(page.items[0].rating.avg().unwrap() > page.items[1].rating.avg().unwrap());

    // Recently verified: never-verified item sorts last (key 0).
    let page = real_search(&request_at(7.0, Sort::RecentlyVerified), 20, false)
        .await
        .unwrap();
    assert!(page.items[2].last_verified_at.is_none());
    cleanup_fixture(MARK).await;
}

#[db_test]
async fn details_assemble_the_full_aggregate(tx: &mut TestTx) {
    const MARK: &str = "fix-details";
    cleanup_fixture(MARK).await;
    let conn = tx.executor();
    let created = at(8.0, 30.0)
        .with_fixture_tag(MARK)
        .with_name("Detalhe Completo")
        .with_cost(Cost::Paid {
            price: Some(Money::new(
                500,
                CurrencyCode::parse("BRL").unwrap(),
                PricingUnit::Day,
            )),
        })
        .with_type(ParkingType::Secured)
        .with_hours(1..=5, (8, 0), (18, 0))
        .with_security("cctv", 1)
        .with_security("indoor", 2) // explicitly no
        .with_security("staffed", 0) // explicitly unknown
        .with_rating(4.2, 5)
        .verified_days_ago(100)
        .create(&mut *conn)
        .await
        .unwrap();
    tx.commit_fixture().await;

    let details = real_details(created.id()).await.unwrap().expect("found");
    assert_eq!(details.name(), "Detalhe Completo");
    assert_eq!(details.parking_type(), ParkingType::Secured);
    match details.cost() {
        Cost::Paid { price: Some(m) } => {
            assert_eq!(m.cents(), 500);
            assert_eq!(m.currency().as_str(), "BRL");
            assert_eq!(m.unit(), PricingUnit::Day);
        }
        other => panic!("expected paid cost, got {other:?}"),
    }
    assert_eq!(details.rating().avg(), Some(4.2));
    assert_eq!(details.rating().count(), 5);
    assert_eq!(
        details.hours(),
        &bikenest_domain::OpeningHours::weekly(
            (1u8..=5)
                .map(|d| {
                    (
                        d,
                        bikenest_domain::TimeRange::new(
                            bikenest_domain::hours::hms(8, 0),
                            bikenest_domain::hours::hms(18, 0),
                        ),
                    )
                })
                .collect()
        )
    );
    let by_code = |c: &str| details.security().iter().find(|f| f.code() == c).unwrap();
    assert_eq!(by_code("cctv").state(), bikenest_domain::SecurityState::Yes);
    assert_eq!(
        by_code("indoor").state(),
        bikenest_domain::SecurityState::No
    );
    assert_eq!(
        by_code("staffed").state(),
        bikenest_domain::SecurityState::Unknown
    );
    // Labels are localized in the presentation layer, not stored — only the
    // code round-trips through the reader.
    assert_eq!(by_code("cctv").code(), "cctv");
    assert_eq!(
        details.security().len(),
        8,
        "every catalog feature recorded"
    );
    assert_eq!(details.security_yes_count(), 1);
    cleanup_fixture(MARK).await;
}

#[db_test]
async fn unknown_hours_map_to_opening_hours_unknown(tx: &mut TestTx) {
    let conn = tx.executor();
    const MARK: &str = "fix-unknown-hours";
    cleanup_fixture(MARK).await;
    let created = at(9.0, 30.0)
        .with_fixture_tag(MARK)
        .with_unknown_hours()
        .create(&mut *conn)
        .await
        .unwrap();
    tx.commit_fixture().await;
    let details = real_details(created.id()).await.unwrap().unwrap();
    assert!(details.hours().is_unknown());
    cleanup_fixture(MARK).await;
}

#[db_test]
async fn details_of_missing_id_is_none(_tx: &mut TestTx) {
    assert!(real_details(987_654_321).await.unwrap().is_none());
}

#[db_test]
async fn invalid_type_code_is_reported_not_silently_mapped(tx: &mut TestTx) {
    const MARK: &str = "fix-badtype";
    cleanup_fixture(MARK).await;
    let conn = tx.executor();
    let id: (i64,) = sqlx::query_as(
        "INSERT INTO parking_location (name, address, parking_type, cost_kind, location, timezone, seed_key) VALUES ('x','y','flying_carpet','free', ST_SetSRID(ST_MakePoint(-46.6,-23.4),4326)::geography, 'America/Sao_Paulo', $1) RETURNING id",
    )
    .bind(MARK)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    tx.commit_fixture().await;
    let _ = test_origin(10.0);

    let err = real_details(id.0).await.unwrap_err();
    assert!(matches!(err, ReaderError::Unexpected(msg) if msg.contains("flying_carpet")));
    cleanup_fixture(MARK).await;
}

// ---------------------------------------------------------------------------
// "Open now": the SQL flag must agree with the domain's opening-hours rule
// ---------------------------------------------------------------------------

/// The search evaluated at a pinned instant, so the SQL wall-clock arithmetic
/// and `OpeningHours::status_at` can be compared on the same input.
async fn search_at(
    request: &SearchRequest,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<SearchPage, ReaderError> {
    let db = bikenest_infrastructure::Db::from_pool(pool().await);
    bikenest_infrastructure::SqlxParkingSearchReader::new(db)
        .search_at(request, 20, false, now)
        .await
}

/// `is_open_now` for the single fixture living in test patch `k`.
async fn sql_open_now(k: f64, now: chrono::DateTime<chrono::Utc>) -> bool {
    let page = search_at(&request_at(k, Sort::Distance), now)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1, "patch {k} holds exactly one fixture");
    page.items[0].is_open_now
}

fn domain_open_now(
    location: &bikenest_domain::ParkingLocation,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    location.hours().status_at(now, location.timezone()) == bikenest_domain::OpenStatus::Open
}

/// A UTC instant from a wall-clock reading in `tz`.
fn instant_at(tz: &str, y: i32, mo: u32, d: u32, h: u32, mi: u32) -> chrono::DateTime<chrono::Utc> {
    use chrono::TimeZone;
    let tz: chrono_tz::Tz = tz.parse().expect("valid timezone");
    tz.with_ymd_and_hms(y, mo, d, h, mi, 0)
        .single()
        .expect("unambiguous local time")
        .with_timezone(&chrono::Utc)
}

fn utc_at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> chrono::DateTime<chrono::Utc> {
    use chrono::TimeZone;
    chrono::Utc
        .with_ymd_and_hms(y, mo, d, h, mi, 0)
        .single()
        .expect("valid instant")
}

#[db_test]
async fn open_now_flag_matches_the_domain_on_a_same_day_range(tx: &mut TestTx) {
    const MARK: &str = "fix-open-same-day";
    cleanup_fixture(MARK).await;
    let conn = tx.executor();
    let spot = at(11.0, 30.0)
        .with_fixture_tag(MARK)
        .with_hours(1..=7, (8, 0), (18, 0))
        .create(&mut *conn)
        .await
        .unwrap();
    tx.commit_fixture().await;

    // Tue 2026-03-10, São Paulo (UTC-3). Both edges of the range, both sides.
    for (h, min, expected) in [(7, 59, false), (8, 0, true), (17, 59, true), (18, 0, false)] {
        let now = instant_at("America/Sao_Paulo", 2026, 3, 10, h, min);
        assert_eq!(
            sql_open_now(11.0, now).await,
            expected,
            "SQL at {h:02}:{min:02} local"
        );
        assert_eq!(
            domain_open_now(&spot, now),
            expected,
            "domain at {h:02}:{min:02} local"
        );
    }
    cleanup_fixture(MARK).await;
}

#[db_test]
async fn open_now_flag_matches_the_domain_across_an_overnight_range(tx: &mut TestTx) {
    const MARK: &str = "fix-open-overnight";
    cleanup_fixture(MARK).await;
    let conn = tx.executor();
    // 22:00 → 02:00: the row belongs to one day but runs into the next.
    let spot = at(12.0, 30.0)
        .with_fixture_tag(MARK)
        .with_hours(1..=7, (22, 0), (2, 0))
        .create(&mut *conn)
        .await
        .unwrap();
    tx.commit_fixture().await;

    for (h, expected) in [(21, false), (23, true), (1, true), (3, false)] {
        let now = instant_at("America/Sao_Paulo", 2026, 3, 10, h, 0);
        assert_eq!(sql_open_now(12.0, now).await, expected, "SQL at {h:02}:00");
        assert_eq!(domain_open_now(&spot, now), expected, "domain at {h:02}:00");
    }
    cleanup_fixture(MARK).await;
}

#[db_test]
async fn open_now_flag_matches_the_domain_across_a_dst_transition(tx: &mut TestTx) {
    const MARK: &str = "fix-open-dst";
    cleanup_fixture(MARK).await;
    let conn = tx.executor();
    // New York springs forward 2026-03-08 at 02:00 local (07:00 UTC).
    let spot = at(13.0, 30.0)
        .with_fixture_tag(MARK)
        .with_timezone("America/New_York")
        .with_hours(1..=7, (3, 0), (6, 0))
        .create(&mut *conn)
        .await
        .unwrap();
    tx.commit_fixture().await;

    // 06:30 UTC is still EST → 01:30 local, before opening.
    let before = utc_at(2026, 3, 8, 6, 30);
    assert!(
        !sql_open_now(13.0, before).await,
        "SQL before the transition"
    );
    assert!(
        !domain_open_now(&spot, before),
        "domain before the transition"
    );

    // 07:30 UTC is EDT → 03:30 local, inside the range.
    let after = utc_at(2026, 3, 8, 7, 30);
    assert!(sql_open_now(13.0, after).await, "SQL after the transition");
    assert!(domain_open_now(&spot, after), "domain after the transition");
    cleanup_fixture(MARK).await;
}

#[db_test]
async fn open_now_filter_uses_the_same_rule_as_the_flag(tx: &mut TestTx) {
    const MARK: &str = "fix-open-filter";
    cleanup_fixture(MARK).await;
    let conn = tx.executor();
    at(14.0, 30.0)
        .with_fixture_tag(MARK)
        .with_name("Overnight")
        .with_hours(1..=7, (22, 0), (2, 0))
        .create(&mut *conn)
        .await
        .unwrap();
    at(14.0, 60.0)
        .with_fixture_tag(MARK)
        .with_name("Daytime")
        .with_hours(1..=7, (8, 0), (18, 0))
        .create(&mut *conn)
        .await
        .unwrap();
    tx.commit_fixture().await;

    let mut req = request_at(14.0, Sort::Distance);
    req.filters.open_now = true;
    // 23:00 in São Paulo: only the overnight location is open.
    let now = instant_at("America/Sao_Paulo", 2026, 3, 10, 23, 0);
    let page = search_at(&req, now).await.unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].name, "Overnight");
    assert!(page.items[0].is_open_now);
    cleanup_fixture(MARK).await;
}

// ---------------------------------------------------------------------------
// Keyset pagination through the use case (cursor built from the SQL sort key)
// ---------------------------------------------------------------------------

/// The real use case over the real reader: it is the use case that turns a
/// row's sort key into the next cursor, so the round trip is what matters.
async fn search_use_case() -> SearchParking {
    let db = bikenest_infrastructure::Db::from_pool(pool().await);
    SearchParking::new(
        Box::new(bikenest_infrastructure::FakeGeocoder),
        Box::new(bikenest_infrastructure::SqlxParkingSearchReader::new(db)),
        bikenest_application::DEFAULT_RECOMMENDATION_CONFIG,
        Default::default(),
    )
}

async fn page_of(k: f64, sort: &str, cursor: Option<String>) -> SearchPage {
    let origin = test_origin(k);
    let (page, _) = search_use_case()
        .await
        .execute(SearchInput {
            lat: Some(origin.lat()),
            lon: Some(origin.lon()),
            radius_m: Some(1000),
            sort: Some(sort.to_string()),
            page_size: Some(20),
            cursor,
            ..Default::default()
        })
        .await
        .expect("search succeeds");
    page
}

/// Walks both pages of a 25-row patch and returns the rows in page order,
/// asserting the page shapes and that the two pages are disjoint.
async fn both_pages(k: f64, sort: &str) -> Vec<bikenest_application::ParkingSummary> {
    let mut first = page_of(k, sort, None).await;
    assert_eq!(first.items.len(), 20, "{sort}: full first page");
    assert_eq!(first.total, 25, "{sort}: total counts every match");
    let cursor = first
        .next_cursor
        .unwrap_or_else(|| panic!("{sort}: a second page exists"));

    let second = page_of(k, sort, Some(cursor.encode())).await;
    assert_eq!(
        second.items.len(),
        5,
        "{sort}: remainder on the second page"
    );
    assert!(
        second.next_cursor.is_none(),
        "{sort}: nothing left after the second page"
    );

    first.items.extend(second.items);
    let mut unique: Vec<i64> = first.items.iter().map(|i| i.id).collect();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 25, "{sort}: no row appears on both pages");
    first.items
}

#[db_test]
async fn recently_verified_pages_advance_instead_of_repeating(tx: &mut TestTx) {
    const MARK: &str = "fix-verified-paging";
    cleanup_fixture(MARK).await;
    let conn = tx.executor();
    for m in 1..=25 {
        at(15.0, f64::from(m) * 10.0)
            .with_fixture_tag(MARK)
            .verified_days_ago(i64::from(m))
            .create(&mut *conn)
            .await
            .unwrap();
    }
    tx.commit_fixture().await;

    let by_verification = both_pages(15.0, "recently_verified").await;
    let verified: Vec<_> = by_verification.iter().map(|i| i.last_verified_at).collect();
    assert!(
        verified.windows(2).all(|w| w[0] >= w[1]),
        "verification timestamps descend across the page boundary"
    );

    // The distance sort paginates over the same fixture.
    let by_distance = both_pages(15.0, "distance").await;
    assert!(
        by_distance
            .windows(2)
            .all(|w| w[0].distance_m <= w[1].distance_m),
        "distances ascend across the page boundary"
    );
    cleanup_fixture(MARK).await;
}
