//! Application-layer tests for `SearchParking` / `GetParkingDetails` using
//! fake ports (no database — the real-SQL tests live in infrastructure).

use bikenest_application::{
    Cursor, Filters, GeoHit, GeocodeError, Geocoder, GetParkingDetails, ParkingDetailsReader,
    ParkingSearchReader, ParkingSummary, ReaderError, SearchError, SearchInput, SearchParking,
};
use bikenest_domain::{Cost, GeoPoint, ParkingLocation, ParkingType, Rating, TimeRange, hms};
use std::sync::{Arc, Mutex};

fn sp_tz() -> chrono_tz::Tz {
    "America/Sao_Paulo".parse().unwrap()
}

fn summary(
    id: i64,
    distance_m: f64,
    rating_avg: Option<f64>,
    verified_days_ago: Option<i64>,
) -> ParkingSummary {
    ParkingSummary {
        id,
        name: format!("Spot {id}"),
        address: format!("Street {id}"),
        parking_type: ParkingType::Rack,
        cost: Cost::Free,
        point: GeoPoint::new(-23.56, -46.65).unwrap(),
        distance_m,
        security_yes: if id % 2 == 0 {
            vec!["cctv", "well_lit"]
        } else {
            vec![]
        }
        .into_iter()
        .map(str::to_string)
        .collect(),
        rating: Rating::new(rating_avg, if rating_avg.is_some() { 3 } else { 0 }).unwrap(),
        last_verified_at: verified_days_ago.map(|d| chrono::Utc::now() - chrono::Duration::days(d)),
        timezone: sp_tz(),
        is_open_now: true,
        photo_key: None,
        // Stands in for the reader's key: these fakes are read with the
        // distance sort, whose key is the distance itself.
        sort_key: Some(distance_m),
    }
}

/// A summary whose sort key is set independently of its distance — what a
/// scoring sort returns.
fn summary_keyed(id: i64, distance_m: f64, sort_key: f64) -> ParkingSummary {
    ParkingSummary {
        sort_key: Some(sort_key),
        ..summary(id, distance_m, None, Some(2))
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
        self.received_apply_cursor
            .lock()
            .unwrap()
            .push(apply_cursor);
        let items = self.items.iter().take(limit).cloned().collect();
        Ok(bikenest_application::SearchPage {
            items,
            total: self.items.len() as i64,
            next_cursor: None,
        })
    }
}

fn use_case(geocoder: FakeGeocoder, reader: FakeReader) -> SearchParking {
    SearchParking::new(Box::new(geocoder), Box::new(reader))
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
    input.sort = Some("distance".to_string());
    input.radius_m = Some(9999); // not in allowlist → default 1000
    input.page_size = Some(100_000); // clamp to MAX_PAGE_SIZE
    let reader = FakeReader::default();
    use_case(FakeGeocoder::default(), reader.clone())
        .execute(input)
        .await
        .unwrap();
    assert_eq!(*reader.received_limit.lock().unwrap().last().unwrap(), 101);

    // Every sort now reads one page plus the look-ahead row, `recommended`
    // (the default) included: there is no candidate fetch to cap any more.
    let rec_input = SearchInput {
        query: Some("Rua XV de Novembro".to_string()),
        page_size: Some(100_000),
        ..Default::default()
    };
    let reader = FakeReader::default();
    use_case(FakeGeocoder::default(), reader.clone())
        .execute(rec_input)
        .await
        .unwrap();
    assert_eq!(*reader.received_limit.lock().unwrap().last().unwrap(), 101);
}

/// The home page's featured strip asks for four cards; the reader must be
/// asked for five (the look-ahead row), not for a five-hundred-row candidate
/// set it would then throw away.
#[tokio::test]
async fn a_small_page_size_reads_a_small_page() {
    let reader = FakeReader::new(
        (1..=30)
            .map(|i| summary(i, i as f64 * 10.0, None, Some(1)))
            .collect(),
    );
    let (page, _) = use_case(FakeGeocoder::default(), reader.clone())
        .execute(SearchInput {
            lat: Some(-25.4297),
            lon: Some(-49.2705),
            radius_m: Some(1000),
            page_size: Some(4),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        *reader.received_limit.lock().unwrap().last().unwrap(),
        5,
        "page_size 4 + one look-ahead row"
    );
    assert_eq!(page.items.len(), 4);
    assert!(page.next_cursor.is_some(), "more rows exist");
}

/// `Recommended` is a SQL sort like every other one now: the reader ranks and
/// limits, the use case passes the cursor down and mints the next one from the
/// key the reader computed. Nothing is re-ranked in memory, so no page can be
/// ordered differently from the query that produced it.
#[tokio::test]
async fn recommended_sort_pages_through_the_reader_like_the_other_sorts() {
    let items = vec![
        summary_keyed(3, 50.0, -0.72),
        summary_keyed(1, 100.0, -0.68),
        summary_keyed(2, 100.0, -0.61),
    ];
    let reader = FakeReader::new(items);
    let mut input = input();
    input.sort = Some("recommended".to_string());
    input.page_size = Some(2);
    let (page, _) = use_case(FakeGeocoder::default(), reader.clone())
        .execute(input.clone())
        .await
        .unwrap();
    assert_eq!(
        reader.received_apply_cursor.lock().unwrap().last().unwrap(),
        &true,
        "the reader applies the keyset predicate for recommended too"
    );
    assert_eq!(reader.received_limit.lock().unwrap().last().unwrap(), &3);
    assert_eq!(
        page.items.iter().map(|i| i.id).collect::<Vec<_>>(),
        vec![3, 1],
        "reader order is the page order"
    );
    let next = page.next_cursor.expect("a second page exists");
    assert_eq!(next.sort, bikenest_application::Sort::Recommended);
    assert!(
        (next.v - (-0.68)).abs() < 1e-9,
        "the cursor anchors on the reader's own sort key, not a recomputed score"
    );
    assert_eq!(next.id, 1);

    // A cursor minted for one sort must not be replayed against another.
    let mut crossed = input;
    crossed.sort = Some("distance".to_string());
    crossed.cursor = Some(next.encode());
    let request = bikenest_application::SearchRequest::new(
        GeoPoint::new(-23.5, -46.6).unwrap(),
        None,
        1000,
        Filters::default(),
        bikenest_application::Sort::Distance,
        20,
        Some(&next.encode()),
    );
    assert!(request.cursor.is_none());
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
    assert_eq!(
        reader.received_apply_cursor.lock().unwrap().last().unwrap(),
        &true
    );
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
    assert_eq!(
        filters.types,
        vec![ParkingType::Rack, ParkingType::Secured, ParkingType::Rack]
    );
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
    assert_eq!(
        request.filters.types,
        vec![ParkingType::Rack, ParkingType::Secured]
    );
    assert_eq!(request.filters.security_all, vec!["cctv", "well_lit"]);
}

// ---------------------------------------------------------------------------
// GetParkingDetails
// ---------------------------------------------------------------------------

/// A filter code no catalog knows can never be confirmed on any location, so
/// keeping it would turn the whole search into "no results". A stale or
/// hand-edited URL degrades to the filters that still mean something.
#[tokio::test]
async fn unknown_security_codes_are_dropped_from_the_request() {
    let request = |codes: Vec<&str>| {
        bikenest_application::SearchRequest::new(
            GeoPoint::new(-23.5, -46.6).unwrap(),
            None,
            1000,
            Filters {
                security_all: codes.into_iter().map(str::to_string).collect(),
                ..Filters::default()
            },
            bikenest_application::Sort::Distance,
            20,
            None,
        )
    };

    assert_eq!(
        request(vec!["cctv", "laser_fence", "well_lit"])
            .filters
            .security_all,
        vec!["cctv", "well_lit"],
    );
    assert!(
        request(vec!["laser_fence"]).filters.security_all.is_empty(),
        "an all-unknown filter set becomes no filter, not an impossible one"
    );
    assert_eq!(
        request(vec![" cctv ", "cctv", ""]).filters.security_all,
        vec!["cctv"],
        "codes are trimmed and deduped"
    );
}

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
    let hours =
        bikenest_domain::OpeningHours::weekly(vec![(1, TimeRange::new(hms(9, 0), hms(18, 0)))]);
    let uc = GetParkingDetails::new(
        Box::new(OneLocationReader(Some(location(hours)))),
        Default::default(),
    );
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
