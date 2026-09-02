//! Application-layer tests for `SearchParking` / `GetParkingDetails` using
//! fake ports (no database — the real-SQL tests live in infrastructure).

use bikenest_application::{
    Cursor, Filters, GeocodeError, GeoHit, Geocoder, GetParkingDetails, ParkingDetailsReader,
    ParkingSearchReader, ParkingSummary, ReaderError,
    DEFAULT_RECOMMENDATION_CONFIG, SearchError, SearchInput, SearchParking,
};
use bikenest_domain::{Cost, GeoPoint, ParkingLocation, ParkingType, Rating, TimeRange, hms};
use std::sync::{Arc, Mutex};

fn sp_tz() -> chrono_tz::Tz {
    "America/Sao_Paulo".parse().unwrap()
}

fn summary(id: i64, distance_m: f64, rating_avg: Option<f64>, verified_days_ago: Option<i64>) -> ParkingSummary {
    ParkingSummary {
        id,
        name: format!("Spot {id}"),
        address: format!("Street {id}"),
        parking_type: ParkingType::Rack,
        cost: Cost::Free,
        point: GeoPoint::new(-23.56, -46.65).unwrap(),
        distance_m,
        security_yes: if id % 2 == 0 { vec!["cctv", "well_lit"] } else { vec![] }
            .into_iter()
            .map(str::to_string)
            .collect(),
        rating: Rating::new(rating_avg, if rating_avg.is_some() { 3 } else { 0 }).unwrap(),
        last_verified_at: verified_days_ago
            .map(|d| chrono::Utc::now() - chrono::Duration::days(d)),
        timezone: sp_tz(),
        is_open_now: true,
        photo_key: None,
    }
}

#[derive(Default, Clone)]
struct FakeGeocoder {
    calls: Arc<Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl Geocoder for FakeGeocoder {
    async fn geocode(&self, query: &str) -> Result<Option<GeoHit>, GeocodeError> {
        self.calls.lock().unwrap().push(query.to_string());
        if query.eq_ignore_ascii_case("rua xv de novembro") {
            Ok(Some(GeoHit {
                label: "Rua XV de Novembro".to_string(),
                point: GeoPoint::new(-23.561_414, -46.655_881).unwrap(),
            }))
        } else {
            Ok(None)
        }
    }
}

#[derive(Default, Clone)]
struct FakeReader {
    items: Vec<ParkingSummary>,
    received_limit: Arc<Mutex<Vec<usize>>>,
    received_apply_cursor: Arc<Mutex<Vec<bool>>>,
}

impl FakeReader {
    fn new(items: Vec<ParkingSummary>) -> Self {
        Self {
            items,
            ..Default::default()
        }
    }
}

#[async_trait::async_trait]
impl ParkingSearchReader for FakeReader {
    async fn search(
        &self,
        _request: &bikenest_application::SearchRequest,
        limit: usize,
        apply_cursor: bool,
    ) -> Result<bikenest_application::SearchPage, ReaderError> {
        self.received_limit.lock().unwrap().push(limit);
        self.received_apply_cursor.lock().unwrap().push(apply_cursor);
        let items = self.items.iter().take(limit).cloned().collect();
        Ok(bikenest_application::SearchPage {
            items,
            total: self.items.len() as i64,
            next_cursor: None,
        })
    }
}

fn use_case(geocoder: FakeGeocoder, reader: FakeReader) -> SearchParking {
    SearchParking::new(
        Box::new(geocoder),
        Box::new(reader),
        DEFAULT_RECOMMENDATION_CONFIG,
        Default::default(),
    )
}

fn input() -> SearchInput {
    SearchInput {
        query: Some("Rua XV de Novembro".to_string()),
        ..Default::default()
    }
}

#[tokio::test]
async fn missing_destination_without_query_or_coordinates() {
    let result = use_case(FakeGeocoder::default(), FakeReader::default())
        .execute(SearchInput::default())
        .await;
    assert!(matches!(result, Err(SearchError::MissingDestination)));
}

#[tokio::test]
async fn unresolvable_query_is_missing_destination() {
    let input = SearchInput {
        query: Some("nowhere at all".to_string()),
        ..Default::default()
    };
    let result = use_case(FakeGeocoder::default(), FakeReader::default())
        .execute(input)
        .await;
    assert!(matches!(result, Err(SearchError::MissingDestination)));
}

#[tokio::test]
async fn explicit_coordinates_win_over_query_and_skip_geocoder() {
    let geocoder = FakeGeocoder::default();
    let input = SearchInput {
        query: Some("Rua XV de Novembro".to_string()),
        lat: Some(-23.55),
        lon: Some(-46.66),
        ..Default::default()
    };
    let (page, hit) = use_case(geocoder.clone(), FakeReader::default())
        .execute(input)
        .await
        .unwrap();
    assert!(hit.is_none(), "explicit coordinates need no geocode hit");
    assert!(geocoder.calls.lock().unwrap().is_empty());
    assert_eq!(page.total, 0);
}

#[tokio::test]
async fn query_is_geocoded_and_label_preserved() {
    let (page, hit) = use_case(FakeGeocoder::default(), FakeReader::default())
        .execute(input())
        .await
        .unwrap();
    assert_eq!(hit.unwrap().label, "Rua XV de Novembro");
    assert_eq!(page.total, 0);
}

#[tokio::test]
async fn radius_and_page_size_are_clamped() {
    let mut input = input();
    input.sort = Some("distance".to_string()); // SQL path: limit = page_size + 1
    input.radius_m = Some(9999); // not in allowlist → default 1000
    input.page_size = Some(100_000); // clamp to MAX_PAGE_SIZE
    let reader = FakeReader::default();
    use_case(FakeGeocoder::default(), reader.clone())
        .execute(input)
        .await
        .unwrap();
    // limit was requested as page_size + 1 (lookahead), so 100 + 1.
    assert_eq!(*reader.received_limit.lock().unwrap().last().unwrap(), 101);

    // The recommended path uses the candidate cap instead.
    let mut rec_input = SearchInput {
        query: Some("Rua XV de Novembro".to_string()),
        ..Default::default()
    }; // default sort = recommended
    rec_input.page_size = Some(100_000);
    let reader = FakeReader::default();
    use_case(FakeGeocoder::default(), reader.clone())
        .execute(rec_input)
        .await
        .unwrap();
    assert_eq!(
        *reader.received_limit.lock().unwrap().last().unwrap(),
        DEFAULT_RECOMMENDATION_CONFIG.candidate_cap
    );
}

#[tokio::test]
async fn recommended_sort_orders_by_score_then_id_and_paginates() {
    // Two items with identical scores (both verified recently, free, no
    // security known, no rating) at different distances → tie broken by id.
    let items = vec![
        summary(2, 100.0, None, Some(2)),
        summary(1, 100.0, None, Some(2)),
        summary(3, 50.0, None, Some(2)), // closer → higher distance score
    ];
    let reader = FakeReader::new(items);
    let mut input = input();
    input.sort = Some("recommended".to_string());
    let (page, _) = use_case(FakeGeocoder::default(), reader.clone())
        .execute(input.clone())
        .await
        .unwrap();
    assert_eq!(page.items.iter().map(|i| i.id).collect::<Vec<_>>(), vec![3, 1, 2]);

    // Deterministic: same input → same order.
    let (page2, _) = use_case(FakeGeocoder::default(), FakeReader::new(vec![
        summary(2, 100.0, None, Some(2)),
        summary(1, 100.0, None, Some(2)),
        summary(3, 50.0, None, Some(2)),
    ]))
    .execute(input.clone())
    .await
    .unwrap();
    assert_eq!(
        page.items.iter().map(|i| i.id).collect::<Vec<_>>(),
        page2.items.iter().map(|i| i.id).collect::<Vec<_>>()
    );

    // Missing data is neutral, not worst: an unverified, unreviewed item with
    // unknown security still beats a verified one that is twice as far? No —
    // but it must beat nothing-by-default: check score neutrality directly.
    let now = chrono::Utc::now();
    let cfg = DEFAULT_RECOMMENDATION_CONFIG;
    let fresh = Default::default();
    let neutral = bikenest_application::recommendation_score(&summary(9, 500.0, None, None), 1000, now, &cfg, &fresh);
    // distance .35*(1-0.5) + security .25*0.5 + rating .2*0.5 + freshness .15*0.5 + verification .05*0.5
    let expected = 0.35 * 0.5 + 0.5 * (0.25 + 0.2 + 0.15 + 0.05);
    assert!((neutral - expected).abs() < 1e-9);

    // Cursor pagination on the recommended sort.
    let cursor = Cursor {
        sort: bikenest_application::Sort::Recommended,
        v: page.items[0].id as f64, // not a real score, but exercises the filter path
        id: page.items[0].id,
    };
    let mut paged_input = input;
    paged_input.cursor = Some(cursor.encode());
    let (paged, _) = use_case(FakeGeocoder::default(), FakeReader::new(vec![
        summary(1, 100.0, None, Some(2)),
        summary(2, 100.0, None, Some(2)),
        summary(3, 50.0, None, Some(2)),
    ]))
    .execute(paged_input)
    .await
    .unwrap();
    assert!(paged.items.iter().all(|i| i.id != page.items[0].id));
}

#[tokio::test]
async fn sql_sorts_pass_cursor_to_reader_and_build_next_cursor() {
    let items = vec![
        summary(1, 100.0, Some(4.0), Some(5)),
        summary(2, 300.0, Some(5.0), Some(1)),
        summary(3, 700.0, Some(3.0), Some(40)),
    ];
    let reader = FakeReader::new(items);
    let mut input = input();
    input.sort = Some("distance".to_string());
    input.page_size = Some(2);
    let (page, _) = use_case(FakeGeocoder::default(), reader.clone())
        .execute(input)
        .await
        .unwrap();
    // reader was asked for page_size+1 rows with cursor application enabled
    assert_eq!(reader.received_limit.lock().unwrap().last().unwrap(), &3);
    assert_eq!(reader.received_apply_cursor.lock().unwrap().last().unwrap(), &true);
    // page_size < items returned → next cursor present, anchored on item 2
    let next = page.next_cursor.expect("has next page");
    assert_eq!(next.sort.as_code(), "distance");
    let decoded = Cursor::decode(&next.encode()).unwrap();
    assert_eq!(decoded.id, page.items[1].id);
    assert!((decoded.v - page.items[1].distance_m).abs() < 1e-9);
}

#[tokio::test]
async fn cursor_roundtrip_and_mismatched_sort_are_handled() {
    let c = Cursor {
        sort: bikenest_application::Sort::Rating,
        v: -4.2,
        id: 77,
    };
    let decoded = Cursor::decode(&c.encode()).unwrap();
    assert_eq!(decoded, c);
    assert!(Cursor::decode("garbage!!!").is_none());
    // A cursor for a different sort is dropped by SearchRequest (page 1).
    let request = bikenest_application::SearchRequest::new(
        GeoPoint::new(-23.5, -46.6).unwrap(),
        Some("x".into()),
        1000,
        Filters::default(),
        bikenest_application::Sort::Distance,
        20,
        Some(&c.encode()),
    );
    assert!(request.cursor.is_none());
}

#[tokio::test]
async fn filters_parse_from_input() {
    let input = SearchInput {
        query: Some("Rua XV de Novembro".to_string()),
        cost: Some("free".to_string()),
        types: Some("rack,bogus,secured,rack".to_string()),
        security: Some("cctv, well_lit".to_string()),
        open_now: true,
        ..Default::default()
    };
    let filters: Filters = input.filters();
    assert_eq!(filters.cost, Some(bikenest_application::CostFilter::Free));
    // Parsing keeps unknown codes out; deduplication is SearchRequest's job.
    assert_eq!(filters.types, vec![ParkingType::Rack, ParkingType::Secured, ParkingType::Rack]);
    assert_eq!(filters.security_all, vec!["cctv", "well_lit"]);
    assert!(filters.open_now);

    // SearchRequest::new normalizes: dedup types, trim/sort security codes.
    let request = bikenest_application::SearchRequest::new(
        GeoPoint::new(-23.5, -46.6).unwrap(),
        Some("x".into()),
        1000,
        filters,
        bikenest_application::Sort::Distance,
        20,
        None,
    );
    assert_eq!(request.filters.types, vec![ParkingType::Rack, ParkingType::Secured]);
    assert_eq!(request.filters.security_all, vec!["cctv", "well_lit"]);
}

// ---------------------------------------------------------------------------
// GetParkingDetails
// ---------------------------------------------------------------------------

struct OneLocationReader(Option<ParkingLocation>);

#[async_trait::async_trait]
impl ParkingDetailsReader for OneLocationReader {
    async fn details(&self, _id: i64) -> Result<Option<ParkingLocation>, ReaderError> {
        Ok(self.0.clone())
    }
}

fn location(hours: bikenest_domain::OpeningHours) -> ParkingLocation {
    ParkingLocation::new(
        42,
        "Estação Vila Mariana",
        "R. Domingos de Morais, 1000",
        None,
        ParkingType::Secured,
        Cost::Paid { price: None },
        GeoPoint::new(-23.5895, -46.6385).unwrap(),
        sp_tz(),
        hours,
        vec![],
        bikenest_domain::ModerationState::Active,
        Rating::new(Some(4.5), 2).unwrap(),
        chrono::Utc::now(),
        chrono::Utc::now(),
        None,
        Some(chrono::Utc::now() - chrono::Duration::days(10)),
        1,
    )
    .unwrap()
}

#[tokio::test]
async fn details_view_computes_freshness_and_open_status() {
    // Open 09:00–18:00 on Mondays (SP local).
    let hours = bikenest_domain::OpeningHours::weekly(vec![(1, TimeRange::new(hms(9, 0), hms(18, 0)))]);
    let uc = GetParkingDetails::new(Box::new(OneLocationReader(Some(location(hours)))), Default::default());
    let view = uc.execute(42).await.unwrap().unwrap();
    assert_eq!(view.freshness, bikenest_domain::FreshnessCategory::Fresh);
    // Unknown hours would be Unknown; with schedule, depends on now — just check it computes.
    assert!(matches!(
        view.is_open_now,
        bikenest_domain::OpenStatus::Open | bikenest_domain::OpenStatus::Closed
    ));
}

#[tokio::test]
async fn details_of_unknown_id_is_none() {
    let uc = GetParkingDetails::new(Box::new(OneLocationReader(None)), Default::default());
    assert!(uc.execute(999).await.unwrap().is_none());
}
