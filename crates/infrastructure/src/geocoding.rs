//! Deterministic fake geocoder (REQUIREMENTS §21; **Ledger #2** — replaced by
//! a real provider in M7).
//!
//! - Exact match (case/accents normalized lightly) against a landmark table.
//! - Fallback: FNV-1a hash of the query → deterministic jitter (±2.5 km)
//!   around a Curitiba centroid, so any search "works" reproducibly in dev.
//! - Empty/whitespace queries → `None` (MissingDestination upstream).

use async_trait::async_trait;
use bikenest_application::{GeocodeError, GeoHit, Geocoder};
use bikenest_domain::GeoPoint;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn landmarks_match_exact_normalized_query() {
        let geo = FakeGeocoder;
        let hit = geo.geocode("Rua XV de Novembro").await.unwrap().unwrap();
        assert!((hit.point.lat() - -25.429_700).abs() < 1e-5);
    }

    #[tokio::test]
    async fn unknown_queries_are_deterministic() {
        let geo = FakeGeocoder;
        let a = geo.geocode("rua sem fim, 123").await.unwrap().unwrap().point;
        let b = geo.geocode("rua sem fim, 123").await.unwrap().unwrap().point;
        assert_eq!(a, b);
        let c = geo.geocode("outra rua, 9").await.unwrap().unwrap().point;
        assert_ne!(a, c);
    }

    #[tokio::test]
    async fn blank_query_returns_none() {
        let geo = FakeGeocoder;
        assert!(geo.geocode("   ").await.unwrap().is_none());
    }
}
