//! Offline, deterministic timezone resolution.
//!
//! **:** this is a coarse bounding-box resolver, not a polygon
//! dataset. It is deliberately small and offline (the "no network in the hot
//! path" rule) and returns a deterministic IANA zone for any point. M7 may
//! replace it with a real geocoder reverse-timezone; the offline resolver stays
//! as the fallback. Points outside the covered boxes fall back to a coarse
//! country-capital table, then to the app's home zone.

use async_trait::async_trait;
use bikesnest_application::{TimezoneError, TimezoneResolver};
use bikesnest_domain::GeoPoint;

/// A bounding box in WGS84 (lat_min, lat_max, lon_min, lon_max).
type BBox = (f64, f64, f64, f64);

/// Coarse, hand-maintained boxes for the zones we care about most. The app is
/// Brazil-focused (seed data is Curitiba / São Paulo), so Brazil gets a large
/// box; the rest of the world is covered by broad regional boxes so a point
/// abroad resolves to *a plausible* zone rather than the Brazil default.
///
/// This is deliberately **not** a polygon dataset (that would bundle hundreds
/// of MB and needs a real polygon engine). It is a bounded, offline,
/// deterministic approximation — any point inside a box maps to that box's
/// zone. Points between boxes fall back to [`DEFAULT_ZONE`]. ****: a
/// real online/polygon reverse-timezone replaces or refines this in M7; keep
/// this as the offline fallback.
const ZONES: &[(BBox, chrono_tz::Tz)] = &[
    // Curitiba / São Paulo / Brasília (the app's home region).
    (
        (-34.0, 5.0, -74.0, -34.0),
        chrono_tz::Tz::America__Sao_Paulo,
    ),
    // Eastern US / Canada.
    ((24.0, 48.0, -82.0, -66.0), chrono_tz::Tz::America__New_York),
    // Central US.
    ((25.0, 48.0, -98.0, -82.0), chrono_tz::Tz::America__Chicago),
    // Western US.
    (
        (32.0, 49.0, -125.0, -98.0),
        chrono_tz::Tz::America__Los_Angeles,
    ),
    // Western Europe.
    ((36.0, 58.0, -12.0, 30.0), chrono_tz::Tz::Europe__Paris),
    // UK / Ireland.
    ((50.0, 59.0, -11.0, 2.0), chrono_tz::Tz::Europe__London),
    // Central Europe.
    ((45.0, 56.0, 6.0, 24.0), chrono_tz::Tz::Europe__Berlin),
    // Moscow / Eastern Europe.
    ((44.0, 66.0, 28.0, 62.0), chrono_tz::Tz::Europe__Moscow),
    // Tokyo / Japan.
    ((30.0, 46.0, 129.0, 146.0), chrono_tz::Tz::Asia__Tokyo),
    // China / East Asia.
    ((18.0, 50.0, 96.0, 135.0), chrono_tz::Tz::Asia__Shanghai),
    // India / South Asia.
    ((6.0, 35.0, 66.0, 92.0), chrono_tz::Tz::Asia__Kolkata),
    // Southeast Asia.
    ((-8.0, 23.0, 92.0, 122.0), chrono_tz::Tz::Asia__Singapore),
    // Eastern Australia.
    (
        (-44.0, -10.0, 113.0, 154.0),
        chrono_tz::Tz::Australia__Sydney,
    ),
    // Mexico / Central America.
    (
        (14.0, 33.0, -118.0, -86.0),
        chrono_tz::Tz::America__Mexico_City,
    ),
    // Andean South America.
    ((-56.0, 2.0, -81.0, -58.0), chrono_tz::Tz::America__Bogota),
];

/// Country-capital boxes for points that fall between the broad zones above.
const CAPITALS: &[(BBox, chrono_tz::Tz)] = &[
    // Portugal (mainland).
    ((36.0, 42.0, -10.0, -6.0), chrono_tz::Tz::Europe__Lisbon),
    // Argentina.
    (
        (-56.0, -21.0, -74.0, -53.0),
        chrono_tz::Tz::America__Argentina__Buenos_Aires,
    ),
    // Chile.
    (
        (-56.0, -17.0, -76.0, -66.0),
        chrono_tz::Tz::America__Santiago,
    ),
    // Iceland / Norway.
    ((60.0, 71.0, -25.0, 30.0), chrono_tz::Tz::Europe__Oslo),
    // Middle East / Turkey.
    ((24.0, 42.0, 26.0, 60.0), chrono_tz::Tz::Asia__Dubai),
    // South Africa.
    (
        (-35.0, -22.0, 16.0, 33.0),
        chrono_tz::Tz::Africa__Johannesburg,
    ),
    // East coast Africa.
    ((-12.0, 36.0, 27.0, 52.0), chrono_tz::Tz::Africa__Nairobi),
    // Philippines.
    ((4.0, 19.0, 118.0, 126.0), chrono_tz::Tz::Asia__Manila),
];

/// The deterministic fallback zone for any point not covered above. Chosen as
/// a widely-recognized zone, but note it may be wrong for a genuinely
/// uncovered point — the offline box table is an approximation.
const DEFAULT_ZONE: chrono_tz::Tz = chrono_tz::Tz::Etc__UTC;

#[derive(Debug, Clone, Copy)]
pub struct OfflineTimezoneResolver;

impl OfflineTimezoneResolver {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OfflineTimezoneResolver {
    fn default() -> Self {
        Self::new()
    }
}

fn in_box(point: GeoPoint, (lat_min, lat_max, lon_min, lon_max): BBox) -> bool {
    point.lat() >= lat_min
        && point.lat() <= lat_max
        && point.lon() >= lon_min
        && point.lon() <= lon_max
}

#[async_trait]
impl TimezoneResolver for OfflineTimezoneResolver {
    async fn resolve(&self, point: GeoPoint) -> Result<chrono_tz::Tz, TimezoneError> {
        let zone = ZONES
            .iter()
            .find(|(bbox, _)| in_box(point, *bbox))
            .map(|(_, z)| *z)
            .or_else(|| {
                CAPITALS
                    .iter()
                    .find(|(bbox, _)| in_box(point, *bbox))
                    .map(|(_, z)| *z)
            })
            .unwrap_or(DEFAULT_ZONE);
        Ok(zone)
    }
}
