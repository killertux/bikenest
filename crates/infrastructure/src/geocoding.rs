//! Geocoders (§21, §84): the deterministic dev `FakeGeocoder` and a real
//! `MapboxGeocoder` (**Ledger #2**; hosted OSM-derived provider, §83). Selected
//! at wiring time by the `GEOCODER` env var (`fake` | `mapbox`) — swapping
//! providers is a config change, never a domain/app change.
//!
//! **§77 (third-party boundary):** the geocoder is called **server-side** with
//! only the free-text destination query. No account identity, cookie, or client
//! IP is sent to the provider; the query string is the complete payload.
//!
//! **§83 — what is documented:** Mapbox Geocoding API (`mapbox.places` forward).
//! Usage is rate/billing-limited by the Mapbox account (`MAPBOX_ACCESS_TOKEN`;
//! the free tier is ~100k requests/month). Terms of service + attribution apply
//! (see docs/provider-transfer-inventory.md —
//! provider contract / DPA / international-transfer review).
//!
//! **Failure mode:** a geocoder error is surfaced to the web layer, which
//! renders a friendly "couldn't reach the geocoder" message (the search handler
//! maps it to a no-results page, never a 500).

use async_trait::async_trait;
use bikenest_application::{GeoHit, GeocodeError, Geocoder};
use bikenest_domain::GeoPoint;

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
// MapboxGeocoder (production, §83)
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

/// Build the Mapbox forward-geocoding URL for a query.
fn mapbox_url(endpoint: &str, token: &str, query: &str, limit: u32) -> String {
    format!(
        "{}/{}.json?access_token={}&limit={limit}",
        endpoint,
        encode_path_segment(query),
        token
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
/// data sent to Mapbox (§77).
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

    pub fn from_env() -> Result<Self, String> {
        std::env::var("MAPBOX_ACCESS_TOKEN")
            .map(Self::new)
            .map_err(|_| "MAPBOX_ACCESS_TOKEN is not set".to_string())
    }
}

#[async_trait]
impl Geocoder for MapboxGeocoder {
    async fn geocode(&self, query: &str) -> Result<Option<GeoHit>, GeocodeError> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(None);
        }
        let url = mapbox_url(&self.endpoint, &self.token, q, 1);
        let bytes = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "Mapbox request failed");
                GeocodeError::Unavailable
            })?
            .error_for_status()
            .map_err(|e| {
                tracing::warn!(error = %e, "Mapbox returned a non-success status");
                GeocodeError::Unexpected(format!("Mapbox status: {e}"))
            })?
            .bytes()
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "Mapbox body read failed");
                GeocodeError::Unavailable
            })?;
        parse_mapbox_response(&bytes)
    }
}

// ---------------------------------------------------------------------------
// Env selection
// ---------------------------------------------------------------------------

/// Build a geocoder from an explicit provider name + optional token. Separated
/// from env so the mapping is unit-testable without mutating process globals.
pub fn geocoder_from(provider: &str, token: Option<&str>) -> Box<dyn Geocoder> {
    match provider.to_ascii_lowercase().as_str() {
        "mapbox" => match token.map(ToOwned::to_owned).map(MapboxGeocoder::new) {
            Some(g) => Box::new(g),
            None => {
                eprintln!("geocoder: MAPBOX_ACCESS_TOKEN is not set; falling back to FakeGeocoder");
                Box::new(FakeGeocoder)
            }
        },
        _ => Box::new(FakeGeocoder),
    }
}

/// Build the geocoder selected by `GEOCODER` (`mapbox` | `fake`, default `fake`),
/// so `cargo run` and the test harness always work without credentials.
pub fn geocoder_from_env() -> Box<dyn Geocoder> {
    let provider = std::env::var("GEOCODER").unwrap_or_else(|_| "fake".to_string());
    let token = std::env::var("MAPBOX_ACCESS_TOKEN").ok();
    geocoder_from(&provider, token.as_deref())
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
        let url = mapbox_url(MAPBOX_ENDPOINT, "tok", "Rua XV, 1", 1);
        assert_eq!(
            url,
            "https://api.mapbox.com/geocoding/v5/mapbox.places/Rua%20XV%2C%201.json?access_token=tok&limit=1"
        );
    }

    // --- env selection -----------------------------------------------------

    #[tokio::test]
    async fn unknown_provider_is_fake() {
        // The returned geocoder must be the deterministic fake (resolves a
        // known landmark to its precise coordinate).
        let geo = geocoder_from("bogus", None);
        let hit = geo.geocode("Rua XV de Novembro").await.unwrap().unwrap();
        assert!((hit.point.lat() - -25.429_700).abs() < 1e-5);
    }
}
