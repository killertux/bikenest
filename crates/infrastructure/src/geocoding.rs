//! Geocoders: the deterministic dev `FakeGeocoder` and a real
//! `MapboxGeocoder` (hosted OSM-derived provider). Selected at wiring time
//! by the parsed `GEOCODER` setting (`fake` | `mapbox`) — swapping providers is
//! a config change, never a domain/app change.
//!
//! ** (third-party boundary):** the geocoder is called **server-side** with
//! only the free-text destination query. No account identity, cookie, or client
//! IP is sent to the provider; the query string is the complete payload.
//!
//! ** — what is documented:** Mapbox Geocoding API (`mapbox.places` forward).
//! Usage is rate/billing-limited by the Mapbox account (`MAPBOX_ACCESS_TOKEN`;
//! the free tier is ~100k requests/month). Terms of service + attribution apply
//! (see docs/provider-transfer-inventory.md —
//! provider contract / DPA / international-transfer review).
//!
//! **Failure mode:** a geocoder error is surfaced to the web layer, which
//! renders a friendly "couldn't reach the geocoder" message (the search handler
//! maps it to a no-results page, never a 500).

use crate::config::GeocoderConfig;

use async_trait::async_trait;
use bikesnest_application::{GeoHit, GeocodeError, Geocoder};
use bikesnest_domain::GeoPoint;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Origin of the home page's featured strip: the Rua XV de Novembro landmark.
/// The strip used to reach it by *geocoding the literal string* on every render
/// — one provider call, billed, to resolve a constant. The home page passes
/// these coordinates instead, which skips the geocoder altogether (explicit
/// coordinates win over a query).
pub const FEATURED_ORIGIN: (f64, f64) = (-25.429_700, -49.270_500);

/// Half-side of the "explore the centre" browse box, in degrees: ±0.02° is
/// ~2.2 km north–south and ~2.0 km east–west at this latitude — a downtown's
/// worth of map, and well inside the browse span limit.
///
/// Every "browse the map" entry point (the nav's parking link, the home page's
/// explore link, the empty-search prompt) is this box around
/// [`FEATURED_ORIGIN`], so they all land on the same view instead of one
/// hard-coded street.
pub const FEATURED_BBOX_HALF_DEG: f64 = 0.02;

// ---------------------------------------------------------------------------
// FakeGeocoder (deterministic, dev/test only)
// ---------------------------------------------------------------------------

const CENTROID: (f64, f64) = (-25.4284, -49.2733);

fn normalize(q: &str) -> String {
    q.trim()
        .to_lowercase()
        .replace(['á', 'à', 'â', 'ã'], "a")
        .replace(['é', 'ê'], "e")
        .replace(['í'], "i")
        .replace(['ó', 'ô', 'õ'], "o")
        .replace(['ú'], "u")
        .replace(['ç'], "c")
}

fn fnv1a(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[derive(Debug, Default, Clone, Copy)]
pub struct FakeGeocoder;

impl FakeGeocoder {
    /// Deterministic jitter: ±2.5 km around the centroid (~0.0225° lat).
    fn fallback(query: &str) -> GeoPoint {
        let h = fnv1a(&normalize(query));
        let dx = ((h % 1000) as f64 / 1000.0 - 0.5) * 0.06; // ≈ ±3.3 km lon
        let dy = ((h / 1000 % 1000) as f64 / 1000.0 - 0.5) * 0.045; // ≈ ±2.5 km lat
        GeoPoint::new(CENTROID.0 + dy, CENTROID.1 + dx).expect("jitter in range")
    }
}

#[async_trait]
impl Geocoder for FakeGeocoder {
    async fn geocode(&self, query: &str) -> Result<Option<GeoHit>, GeocodeError> {
        let q = normalize(query);
        if q.is_empty() {
            return Ok(None);
        }
        if let Some((_, lat, lon)) = crate::devdata::LANDMARKS
            .iter()
            .find(|(name, _, _)| *name == q)
        {
            return Ok(Some(GeoHit {
                label: query.trim().to_string(),
                point: GeoPoint::new(*lat, *lon).expect("landmark in range"),
            }));
        }
        Ok(Some(GeoHit {
            label: query.trim().to_string(),
            point: Self::fallback(query),
        }))
    }
}

// ---------------------------------------------------------------------------
// MapboxGeocoder (production)
// ---------------------------------------------------------------------------

/// Default Mapbox Geocoding v5 forward endpoint (`mapbox.places`).
const MAPBOX_ENDPOINT: &str = "https://api.mapbox.com/geocoding/v5/mapbox.places";

/// Percent-encode a string for a single URL path segment (RFC 3986 unreserved
/// set). Spaces, commas, slashes and `&` (all legal in a destination query)
/// become `%XX`, so the query survives as one clean path segment.
fn encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Build the Mapbox forward-geocoding URL for a query. Deliberately excludes
/// the access token: it is attached separately via the request builder's
/// `.query(...)` so it never appears in a URL we might log or format into an
/// error message (`reqwest::Error`'s `Display` embeds the full request URL).
fn mapbox_url(endpoint: &str, query: &str, limit: u32) -> String {
    format!(
        "{}/{}.json?limit={limit}",
        endpoint,
        encode_path_segment(query)
    )
}

/// Summarize a `reqwest::Error` for logs/error messages using only structured
/// facts (HTTP status, timeout, connect-failure) — never the error's own
/// `Display`/`{:?}`, which embeds the full request URL and would leak the
/// Mapbox access token carried as a query parameter.
fn describe_reqwest_error(e: &reqwest::Error) -> String {
    format!(
        "status={:?} timeout={} connect={}",
        e.status().map(|s| s.as_u16()),
        e.is_timeout(),
        e.is_connect()
    )
}

#[derive(serde::Deserialize)]
struct MapboxResponse {
    #[serde(default)]
    features: Vec<MapboxFeature>,
}

#[derive(serde::Deserialize)]
struct MapboxFeature {
    /// `[lon, lat]` in GeoJSON order.
    center: Option<[f64; 2]>,
    place_name: Option<String>,
    text: Option<String>,
}

/// Parse a Mapbox geocoding response into the best [`GeoHit`]. Empty/featureless
/// responses → `Ok(None)` (a genuinely unresolvable destination).
fn parse_mapbox_response(bytes: &[u8]) -> Result<Option<GeoHit>, GeocodeError> {
    let resp: MapboxResponse = serde_json::from_slice(bytes)
        .map_err(|e| GeocodeError::Unexpected(format!("bad Mapbox response: {e}")))?;
    for f in resp.features {
        if let Some([lon, lat]) = f.center
            && let Ok(point) = GeoPoint::new(lat, lon)
        {
            let label = f
                .place_name
                .clone()
                .or_else(|| f.text.clone())
                .unwrap_or_else(|| format!("{lat}, {lon}"));
            return Ok(Some(GeoHit { label, point }));
        }
    }
    Ok(None)
}

/// Real Mapbox geocoder. Caller holds the access token; the query is the only
/// data sent to Mapbox.
pub struct MapboxGeocoder {
    client: reqwest::Client,
    token: String,
    endpoint: String,
}

impl MapboxGeocoder {
    pub fn new(token: impl Into<String>) -> Self {
        // 5s timeout: geocoding runs inline during a user search; don't let a
        // slow Mapbox response hang the page.
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("reqwest client");
        Self {
            client,
            token: token.into(),
            endpoint: MAPBOX_ENDPOINT.to_string(),
        }
    }
}

#[async_trait]
impl Geocoder for MapboxGeocoder {
    async fn geocode(&self, query: &str) -> Result<Option<GeoHit>, GeocodeError> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(None);
        }
        let url = mapbox_url(&self.endpoint, q, 1);
        let bytes = self
            .client
            .get(&url)
            .query(&[("access_token", &self.token)])
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(mapbox_error = %describe_reqwest_error(&e), "Mapbox request failed");
                GeocodeError::Unavailable
            })?
            .error_for_status()
            .map_err(|e| {
                let desc = describe_reqwest_error(&e);
                tracing::warn!(mapbox_error = %desc, "Mapbox returned a non-success status");
                GeocodeError::Unexpected(format!("Mapbox status: {desc}"))
            })?
            .bytes()
            .await
            .map_err(|e| {
                tracing::warn!(mapbox_error = %describe_reqwest_error(&e), "Mapbox body read failed");
                GeocodeError::Unavailable
            })?;
        parse_mapbox_response(&bytes)
    }
}

// ---------------------------------------------------------------------------
// Selection
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// CachingGeocoder
// ---------------------------------------------------------------------------

/// How long a resolved destination is reused. A street address does not move,
/// and a day is short enough that a provider correction reaches us quickly.
pub const GEOCODE_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Upper bound on remembered queries — this is a bounded cache, not a leak
/// that grows with every distinct string anyone types into the search box.
/// ~10k entries of (query, label, point) is well under a megabyte.
pub const GEOCODE_CACHE_CAPACITY: usize = 10_000;

/// In-process cache in front of a real geocoder.
///
/// Every geocode is a billable third-party call, and the search box
/// resolves the same few dozen destinations over and over: "the city centre",
/// a shared link making the rounds, one person paging through results. Without
/// this, each of those is a fresh call to the provider.
///
/// Deliberately in-process, and therefore per-instance: it needs no store, no
/// eviction daemon and no failure mode of its own. A shared ValKey tier (the
/// rate limiter already talks to one) would let several instances share the
/// savings and survive a restart — the obvious follow-up, and the reason
/// [`Self::peek`] exists as a lookup rather than the cache being hidden inside
/// [`Geocoder::geocode`].
///
/// Only *resolved* queries are remembered. A query the provider could not
/// resolve is not cached: those are typos and junk, they must not pin a
/// failure for a day, and the per-IP budget in the web layer is what stops
/// anyone hammering the provider with them.
pub struct CachingGeocoder {
    inner: Box<dyn Geocoder>,
    state: Mutex<Cache>,
    ttl: Duration,
    capacity: usize,
}

struct Cache {
    hits: HashMap<String, (GeoHit, Instant)>,
    /// Keys in insertion order: the bound evicts the oldest.
    order: VecDeque<String>,
}

impl CachingGeocoder {
    pub fn new(inner: Box<dyn Geocoder>) -> Self {
        Self::with_limits(inner, GEOCODE_CACHE_TTL, GEOCODE_CACHE_CAPACITY)
    }

    fn with_limits(inner: Box<dyn Geocoder>, ttl: Duration, capacity: usize) -> Self {
        Self {
            inner,
            state: Mutex::new(Cache {
                hits: HashMap::new(),
                order: VecDeque::new(),
            }),
            ttl,
            capacity,
        }
    }

    /// The cached resolution for `query`, if one is live — without calling the
    /// provider and without recording anything.
    ///
    /// The web layer checks this before charging a search against the caller's
    /// geocode budget: a query this cache can already answer costs the
    /// provider nothing, so it must not cost the caller anything either.
    pub fn peek(&self, query: &str) -> Option<GeoHit> {
        let key = normalize(query);
        let state = self.state.lock().expect("geocode cache mutex");
        let (hit, at) = state.hits.get(&key)?;
        (at.elapsed() < self.ttl).then(|| hit.clone())
    }

    fn remember(&self, query: &str, hit: &GeoHit) {
        let key = normalize(query);
        let mut state = self.state.lock().expect("geocode cache mutex");
        if state
            .hits
            .insert(key.clone(), (hit.clone(), Instant::now()))
            .is_none()
        {
            state.order.push_back(key);
        }
        while state.order.len() > self.capacity {
            if let Some(oldest) = state.order.pop_front() {
                state.hits.remove(&oldest);
            }
        }
    }
}

#[async_trait]
impl Geocoder for CachingGeocoder {
    async fn geocode(&self, query: &str) -> Result<Option<GeoHit>, GeocodeError> {
        if let Some(hit) = self.peek(query) {
            return Ok(Some(hit));
        }
        let resolved = self.inner.geocode(query).await?;
        if let Some(hit) = &resolved {
            self.remember(query, hit);
        }
        Ok(resolved)
    }
}

/// One [`CachingGeocoder`] behind the [`Geocoder`] port, so the use case and
/// the handler that inspects the cache hold the same instance (the same shape
/// as `SharedRateLimiter`).
pub struct SharedGeocoder(Arc<CachingGeocoder>);

impl SharedGeocoder {
    pub fn new(inner: Arc<CachingGeocoder>) -> Self {
        Self(inner)
    }
}

#[async_trait]
impl Geocoder for SharedGeocoder {
    async fn geocode(&self, query: &str) -> Result<Option<GeoHit>, GeocodeError> {
        self.0.geocode(query).await
    }
}

/// Build the geocoder the parsed configuration selected. `Mapbox` carries its
/// token, so there is no "asked for Mapbox, got the fake" path any more: a
/// missing token is rejected while the configuration is parsed.
pub fn geocoder_from_config(config: &GeocoderConfig) -> Box<dyn Geocoder> {
    match config {
        GeocoderConfig::Fake => Box::new(FakeGeocoder),
        GeocoderConfig::Mapbox { token } => Box::new(MapboxGeocoder::new(token.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- FakeGeocoder ------------------------------------------------------

    #[tokio::test]
    async fn landmarks_match_exact_normalized_query() {
        let geo = FakeGeocoder;
        let hit = geo.geocode("Rua XV de Novembro").await.unwrap().unwrap();
        assert!((hit.point.lat() - -25.429_700).abs() < 1e-5);
    }

    #[tokio::test]
    async fn unknown_queries_are_deterministic() {
        let geo = FakeGeocoder;
        let a = geo
            .geocode("rua sem fim, 123")
            .await
            .unwrap()
            .unwrap()
            .point;
        let b = geo
            .geocode("rua sem fim, 123")
            .await
            .unwrap()
            .unwrap()
            .point;
        assert_eq!(a, b);
        let c = geo.geocode("outra rua, 9").await.unwrap().unwrap().point;
        assert_ne!(a, c);
    }

    #[tokio::test]
    async fn blank_query_returns_none() {
        let geo = FakeGeocoder;
        assert!(geo.geocode("   ").await.unwrap().is_none());
    }

    // --- Mapbox parsing ----------------------------------------------------

    #[test]
    fn parses_best_feature_into_geohit() {
        let body = br#"{
          "features": [
            { "center": [-49.2733, -25.4284], "place_name": "Rua XV de Novembro, Curitiba, PR, Brazil", "text": "Rua XV de Novembro" },
            { "center": [-49.2700, -25.4200], "place_name": "other, Brazil", "text": "other" }
          ]
        }"#;
        let hit = parse_mapbox_response(body).unwrap().unwrap();
        assert_eq!(hit.label, "Rua XV de Novembro, Curitiba, PR, Brazil");
        assert!((hit.point.lat() - -25.4284).abs() < 1e-9);
        assert!((hit.point.lon() - -49.2733).abs() < 1e-9);
    }

    #[test]
    fn empty_features_mean_not_found() {
        assert!(
            parse_mapbox_response(br#"{"features":[]}"#)
                .unwrap()
                .is_none()
        );
        assert!(
            parse_mapbox_response(br#"{"query":["x"]}"#)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn falls_back_to_text_when_no_place_name() {
        let body =
            br#"{ "features": [ { "center": [-49.2733, -25.4284], "text": "Only text" } ] }"#;
        let hit = parse_mapbox_response(body).unwrap().unwrap();
        assert_eq!(hit.label, "Only text");
    }

    #[test]
    fn ignores_features_without_center() {
        let body = br#"{ "features": [ { "place_name": "no coords" } ] }"#;
        assert!(parse_mapbox_response(body).unwrap().is_none());
    }

    // --- URL / encoding ----------------------------------------------------

    #[test]
    fn encodes_query_for_path_segment() {
        assert_eq!(
            encode_path_segment("Rua XV de Novembro, 123"),
            "Rua%20XV%20de%20Novembro%2C%20123"
        );
        assert_eq!(encode_path_segment("A & B / C"), "A%20%26%20B%20%2F%20C");
    }

    #[test]
    fn builds_mapbox_url() {
        let url = mapbox_url(MAPBOX_ENDPOINT, "Rua XV, 1", 1);
        assert_eq!(
            url,
            "https://api.mapbox.com/geocoding/v5/mapbox.places/Rua%20XV%2C%201.json?limit=1"
        );
        assert!(
            !url.contains("access_token"),
            "the token must never be part of the formatted URL"
        );
    }

    // --- error mapping never leaks the access token -------------------------

    #[tokio::test]
    async fn describe_reqwest_error_never_contains_the_token() {
        // Nothing listens on this loopback port, so the request fails fast with
        // a connect error — and `reqwest::Error`'s own Display embeds the full
        // request URL (and thus the token carried in its query string), which
        // is exactly what `describe_reqwest_error` must never reproduce.
        let token = "super-secret-mapbox-token";
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(200))
            .build()
            .unwrap();
        let err = client
            .get(format!("http://127.0.0.1:1/geocode?access_token={token}"))
            .send()
            .await
            .expect_err("connecting to a closed port must fail");
        assert!(
            format!("{err}").contains(token),
            "sanity check: reqwest's own Display does leak the token"
        );
        let described = describe_reqwest_error(&err);
        assert!(
            !described.contains(token),
            "error summary must never contain the token: {described}"
        );
    }

    #[tokio::test]
    async fn geocode_error_never_leaks_the_token() {
        let token = "super-secret-mapbox-token";
        // Nothing listens on this loopback port: every request is a fast
        // connect failure, and the URL passed to `.get()` would carry the
        // token if `mapbox_url` still embedded it.
        let geo = MapboxGeocoder {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(200))
                .build()
                .unwrap(),
            token: token.to_string(),
            endpoint: "http://127.0.0.1:1".to_string(),
        };
        let err = geo
            .geocode("Rua XV de Novembro")
            .await
            .expect_err("connecting to a closed port must fail");
        let rendered = format!("{err:?}");
        assert!(
            !rendered.contains(token),
            "GeocodeError must never contain the token: {rendered}"
        );
    }

    // --- selection ---------------------------------------------------------

    /// A provider whose calls are counted, so a test can prove one did *not*
    /// happen. The counter is shared, not owned: the cache takes the geocoder.
    #[derive(Default)]
    struct Calls(std::sync::atomic::AtomicUsize);

    impl Calls {
        fn get(&self) -> usize {
            self.0.load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    struct CountingGeocoder {
        calls: Arc<Calls>,
        resolves: bool,
    }

    /// A counted provider plus the handle to read its tally.
    fn counting(resolves: bool) -> (Box<dyn Geocoder>, Arc<Calls>) {
        let calls = Arc::new(Calls::default());
        (
            Box::new(CountingGeocoder {
                calls: calls.clone(),
                resolves,
            }),
            calls,
        )
    }

    #[async_trait]
    impl Geocoder for CountingGeocoder {
        async fn geocode(&self, query: &str) -> Result<Option<GeoHit>, GeocodeError> {
            self.calls
                .0
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if !self.resolves {
                return Ok(None);
            }
            Ok(Some(GeoHit {
                label: query.trim().to_string(),
                point: GeoPoint::new(-25.4297, -49.2705).unwrap(),
            }))
        }
    }

    #[tokio::test]
    async fn a_resolved_query_is_answered_from_the_cache_next_time() {
        let (inner, calls) = counting(true);
        let geo = CachingGeocoder::new(inner);

        let first = geo.geocode("Rua XV de Novembro").await.unwrap().unwrap();
        // Same destination, differently typed: the key is the normalized query.
        let second = geo.geocode("  rua xv de novembro ").await.unwrap().unwrap();
        assert_eq!(first.point, second.point);
        assert_eq!(calls.get(), 1, "the provider is paid once");
        assert!(geo.peek("RUA XV DE NOVEMBRO").is_some());
        assert!(geo.peek("somewhere else").is_none());
    }

    #[tokio::test]
    async fn a_query_that_does_not_resolve_is_not_remembered() {
        let (inner, calls) = counting(false);
        let geo = CachingGeocoder::new(inner);

        assert!(geo.geocode("asdfghjkl").await.unwrap().is_none());
        assert!(geo.peek("asdfghjkl").is_none(), "no failure is pinned");
        assert!(geo.geocode("asdfghjkl").await.unwrap().is_none());
        assert_eq!(calls.get(), 2);
    }

    #[tokio::test]
    async fn an_entry_expires_and_is_resolved_again() {
        let (inner, calls) = counting(true);
        let geo =
            CachingGeocoder::with_limits(inner, Duration::from_millis(40), GEOCODE_CACHE_CAPACITY);

        geo.geocode("Praça Tiradentes").await.unwrap();
        assert!(geo.peek("Praça Tiradentes").is_some());
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(geo.peek("Praça Tiradentes").is_none(), "the entry aged out");
        geo.geocode("Praça Tiradentes").await.unwrap();
        assert_eq!(calls.get(), 2);
    }

    #[tokio::test]
    async fn the_cache_is_bounded_and_evicts_the_oldest_entry() {
        let (inner, _calls) = counting(true);
        let geo = CachingGeocoder::with_limits(inner, GEOCODE_CACHE_TTL, 2);

        for q in ["first", "second", "third"] {
            geo.geocode(q).await.unwrap();
        }
        assert!(geo.peek("first").is_none(), "the oldest entry was evicted");
        assert!(geo.peek("second").is_some());
        assert!(geo.peek("third").is_some());
        assert_eq!(
            geo.state.lock().unwrap().hits.len(),
            2,
            "the map never grows past the bound"
        );

        // Re-resolving a live entry refreshes it in place rather than
        // consuming another slot.
        geo.geocode("second").await.unwrap();
        assert_eq!(geo.state.lock().unwrap().order.len(), 2);
    }

    #[test]
    fn the_featured_origin_is_the_rua_xv_landmark() {
        let (_, lat, lon) = crate::devdata::LANDMARKS
            .iter()
            .find(|(name, _, _)| *name == "rua xv de novembro")
            .expect("the landmark the home page features");
        assert_eq!((*lat, *lon), FEATURED_ORIGIN);
    }

    #[tokio::test]
    async fn fake_config_builds_the_deterministic_fake() {
        // The returned geocoder must be the deterministic fake (resolves a
        // known landmark to its precise coordinate).
        let geo = geocoder_from_config(&GeocoderConfig::Fake);
        let hit = geo.geocode("Rua XV de Novembro").await.unwrap().unwrap();
        assert!((hit.point.lat() - -25.429_700).abs() < 1e-5);
    }
}
