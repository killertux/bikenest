//! Parking location domain model (REQUIREMENTS §24–§29, §40).

use crate::{DomainError, UserId};
use chrono::{DateTime, Utc};

// ---------------------------------------------------------------------------
// Geo
// ---------------------------------------------------------------------------

/// WGS84 point with validation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoPoint {
    lat: f64,
    lon: f64,
}

impl GeoPoint {
    pub fn new(lat: f64, lon: f64) -> Result<Self, DomainError> {
        if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
            return Err(DomainError::Invalid("coordinates out of range".to_string()));
        }
        Ok(Self { lat, lon })
    }

    pub fn lat(&self) -> f64 {
        self.lat
    }

    pub fn lon(&self) -> f64 {
        self.lon
    }

    /// Great-circle distance in meters (haversine).
    pub fn distance_m_to(&self, other: &GeoPoint) -> f64 {
        const EARTH_R: f64 = 6_371_000.0;
        let (lat1, lat2) = (self.lat.to_radians(), other.lat.to_radians());
        let dlat = lat2 - lat1;
        let dlon = (other.lon - self.lon).to_radians();
        let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
        2.0 * EARTH_R * a.sqrt().asin()
    }
}

// ---------------------------------------------------------------------------
// Type (§26)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParkingType {
    Rack,
    ParkingFacility,
    Indoor,
    Secured,
    Locker,
    Other,
}

impl ParkingType {
    pub const ALL: &'static [ParkingType] = &[
        ParkingType::Rack,
        ParkingType::ParkingFacility,
        ParkingType::Indoor,
        ParkingType::Secured,
        ParkingType::Locker,
        ParkingType::Other,
    ];

    pub fn as_code(&self) -> &'static str {
        match self {
            ParkingType::Rack => "rack",
            ParkingType::ParkingFacility => "parking_facility",
            ParkingType::Indoor => "indoor",
            ParkingType::Secured => "secured",
            ParkingType::Locker => "locker",
            ParkingType::Other => "other",
        }
    }

    /// Unknown codes are rejected explicitly, never silently mapped to Other.
    pub fn from_code(code: &str) -> Result<Self, DomainError> {
        match code {
            "rack" => Ok(ParkingType::Rack),
            "parking_facility" => Ok(ParkingType::ParkingFacility),
            "indoor" => Ok(ParkingType::Indoor),
            "secured" => Ok(ParkingType::Secured),
            "locker" => Ok(ParkingType::Locker),
            "other" => Ok(ParkingType::Other),
            other => Err(DomainError::Invalid(format!(
                "unknown parking type: {other}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Cost (§27)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PricingUnit {
    Hour,
    Day,
    Month,
    Entry,
}

impl PricingUnit {
    pub fn as_code(&self) -> &'static str {
        match self {
            PricingUnit::Hour => "hour",
            PricingUnit::Day => "day",
            PricingUnit::Month => "month",
            PricingUnit::Entry => "entry",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, DomainError> {
        match code {
            "hour" => Ok(PricingUnit::Hour),
            "day" => Ok(PricingUnit::Day),
            "month" => Ok(PricingUnit::Month),
            "entry" => Ok(PricingUnit::Entry),
            other => Err(DomainError::Invalid(format!(
                "unknown pricing unit: {other}"
            ))),
        }
    }
}

/// ISO-4217 style currency code (§27: "ISO-compatible currency representation").
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CurrencyCode(String);

impl CurrencyCode {
    pub fn parse(code: &str) -> Result<Self, DomainError> {
        let code = code.trim().to_ascii_uppercase();
        if code.len() == 3 && code.chars().all(|c| c.is_ascii_alphabetic()) {
            Ok(Self(code))
        } else {
            Err(DomainError::Invalid(format!(
                "invalid currency code: {code}"
            )))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Amount in minor units (cents) — no floating point money.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Money {
    cents: i64,
    currency: CurrencyCode,
    unit: PricingUnit,
}

impl Money {
    pub fn new(cents: i64, currency: CurrencyCode, unit: PricingUnit) -> Self {
        Self {
            cents,
            currency,
            unit,
        }
    }

    pub fn cents(&self) -> i64 {
        self.cents
    }

    pub fn currency(&self) -> &CurrencyCode {
        &self.currency
    }

    pub fn unit(&self) -> PricingUnit {
        self.unit
    }
}

/// The three-way distinction from §27. Note: `Paid` with `price: None` models
/// "paid, but the price is not currently known" — distinct from `Unknown`.
#[derive(Debug, Clone, PartialEq)]
pub enum Cost {
    Free,
    Paid { price: Option<Money> },
    Unknown,
}

impl Cost {
    pub fn kind_code(&self) -> &'static str {
        match self {
            Cost::Free => "free",
            Cost::Paid { .. } => "paid",
            Cost::Unknown => "unknown",
        }
    }

    pub fn from_kind_and_price(kind: &str, price: Option<Money>) -> Result<Self, DomainError> {
        match kind {
            "free" => Ok(Cost::Free),
            "paid" => Ok(Cost::Paid { price }),
            "unknown" => Ok(Cost::Unknown),
            other => Err(DomainError::Invalid(format!("unknown cost kind: {other}"))),
        }
    }
}

// ---------------------------------------------------------------------------
// Security (§28)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityState {
    /// Unknown — explicitly not the same as "no" (§28).
    Unknown,
    Yes,
    No,
}

impl SecurityState {
    pub fn from_smallint(v: i16) -> Result<Self, DomainError> {
        match v {
            0 => Ok(SecurityState::Unknown),
            1 => Ok(SecurityState::Yes),
            2 => Ok(SecurityState::No),
            other => Err(DomainError::Invalid(format!(
                "unknown security state: {other}"
            ))),
        }
    }
}

/// Canonical security-attribute codes (§28). Labels are **localized in the
/// presentation layer** (i18n), not stored, so the catalog is a hardcoded list
/// here rather than a database table — new features add a code + translations,
/// no migration. Also used for the recommendation score's denominator.
pub const SECURITY_FEATURE_CODES: &[&str] = &[
    "dedicated_locking_point",
    "indoor",
    "cctv",
    "staffed",
    "security_guard",
    "controlled_access",
    "well_lit",
    "restricted_access",
];

/// Whether `code` is a recognized security attribute (§28).
pub fn is_known_security_code(code: &str) -> bool {
    SECURITY_FEATURE_CODES.contains(&code)
}

/// One security attribute of a location. The human-readable label is resolved
/// from `code` in the presentation layer (i18n), so it is not stored here.
#[derive(Debug, Clone, PartialEq)]
pub struct SecurityFeature {
    code: String,
    state: SecurityState,
}

impl SecurityFeature {
    pub fn new(code: impl Into<String>, state: SecurityState) -> Self {
        Self {
            code: code.into(),
            state,
        }
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn state(&self) -> SecurityState {
        self.state
    }
}

// ---------------------------------------------------------------------------
// Moderation state (§25)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModerationState {
    Active,
    PendingReview,
    Flagged,
    Invalid,
    Removed,
}

impl ModerationState {
    pub fn as_code(self) -> &'static str {
        match self {
            ModerationState::Active => "ACTIVE",
            ModerationState::PendingReview => "PENDING_REVIEW",
            ModerationState::Flagged => "FLAGGED",
            ModerationState::Invalid => "INVALID",
            ModerationState::Removed => "REMOVED",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, DomainError> {
        match code {
            "ACTIVE" => Ok(ModerationState::Active),
            "PENDING_REVIEW" => Ok(ModerationState::PendingReview),
            "FLAGGED" => Ok(ModerationState::Flagged),
            "INVALID" => Ok(ModerationState::Invalid),
            "REMOVED" => Ok(ModerationState::Removed),
            other => Err(DomainError::Invalid(format!(
                "unknown moderation state: {other}"
            ))),
        }
    }

    /// Public search only ever returns Active (§25).
    pub fn is_publicly_visible(&self) -> bool {
        matches!(self, ModerationState::Active)
    }
}

// ---------------------------------------------------------------------------
// Rating
// ---------------------------------------------------------------------------

/// Denormalized review aggregate. Reviews themselves arrive in M3; the seeder
/// fills this for M1 demo data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rating {
    avg: Option<f64>,
    count: i64,
}

impl Rating {
    pub fn new(avg: Option<f64>, count: i64) -> Result<Self, DomainError> {
        if let Some(avg) = avg
            && !(0.0..=5.0).contains(&avg)
        {
            return Err(DomainError::Invalid("rating avg out of range".to_string()));
        }
        if count < 0 {
            return Err(DomainError::Invalid("negative rating count".to_string()));
        }
        Ok(Self {
            avg: if count == 0 { None } else { avg },
            count,
        })
    }

    pub fn avg(&self) -> Option<f64> {
        self.avg
    }

    pub fn count(&self) -> i64 {
        self.count
    }
}

// ---------------------------------------------------------------------------
// ParkingLocation aggregate (§24)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ParkingLocation {
    id: i64,
    name: String,
    address: String,
    description: Option<String>,
    parking_type: ParkingType,
    cost: Cost,
    point: GeoPoint,
    timezone: chrono_tz::Tz,
    hours: crate::hours::OpeningHours,
    security: Vec<SecurityFeature>,
    moderation_state: ModerationState,
    rating: Rating,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    last_meaningful_update_at: Option<DateTime<Utc>>,
    last_verified_at: Option<DateTime<Utc>>,
    /// Optimistic-concurrency version (§100). Starts at 1; bumped on each applied edit.
    version: i64,
}

impl ParkingLocation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: i64,
        name: impl Into<String>,
        address: impl Into<String>,
        description: Option<String>,
        parking_type: ParkingType,
        cost: Cost,
        point: GeoPoint,
        timezone: chrono_tz::Tz,
        hours: crate::hours::OpeningHours,
        security: Vec<SecurityFeature>,
        moderation_state: ModerationState,
        rating: Rating,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        last_meaningful_update_at: Option<DateTime<Utc>>,
        last_verified_at: Option<DateTime<Utc>>,
        version: i64,
    ) -> Result<Self, DomainError> {
        let name = name.into().trim().to_string();
        if name.is_empty() {
            return Err(DomainError::Invalid("name is required".to_string()));
        }
        let address = address.into().trim().to_string();
        if address.is_empty() {
            return Err(DomainError::Invalid("address is required".to_string()));
        }
        Ok(Self {
            id,
            name,
            address,
            description,
            parking_type,
            cost,
            point,
            timezone,
            hours,
            security,
            moderation_state,
            rating,
            created_at,
            updated_at,
            last_meaningful_update_at,
            last_verified_at,
            version,
        })
    }

    pub fn id(&self) -> i64 {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn address(&self) -> &str {
        &self.address
    }
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
    pub fn parking_type(&self) -> ParkingType {
        self.parking_type
    }
    pub fn cost(&self) -> &Cost {
        &self.cost
    }
    pub fn point(&self) -> &GeoPoint {
        &self.point
    }
    pub fn timezone(&self) -> chrono_tz::Tz {
        self.timezone
    }
    pub fn hours(&self) -> &crate::hours::OpeningHours {
        &self.hours
    }
    pub fn security(&self) -> &[SecurityFeature] {
        &self.security
    }
    pub fn moderation_state(&self) -> ModerationState {
        self.moderation_state
    }
    pub fn rating(&self) -> &Rating {
        &self.rating
    }
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
    pub fn last_meaningful_update_at(&self) -> Option<DateTime<Utc>> {
        self.last_meaningful_update_at
    }
    pub fn last_verified_at(&self) -> Option<DateTime<Utc>> {
        self.last_verified_at
    }
    pub fn version(&self) -> i64 {
        self.version
    }

    /// Count of security attributes explicitly confirmed present (§28: yes-state).
    pub fn security_yes_count(&self) -> usize {
        self.security
            .iter()
            .filter(|f| f.state() == SecurityState::Yes)
            .count()
    }
}

/// Author of a contribution — placeholder for M3 write flows; used by seeds.
#[derive(Debug, Clone, Copy)]
pub struct Contributor(pub UserId);

#[cfg(test)]
mod tests {
    use super::*;

    // --- Cost tri-state (§27) --------------------------------------------

    #[test]
    fn cost_kinds_round_trip_through_kind_code() {
        assert_eq!(Cost::Free.kind_code(), "free");
        assert_eq!(Cost::Unknown.kind_code(), "unknown");
        assert_eq!(Cost::Paid { price: None }.kind_code(), "paid");
        let money = Money::new(500, CurrencyCode::parse("BRL").unwrap(), PricingUnit::Day);
        assert_eq!(Cost::Paid { price: Some(money) }.kind_code(), "paid");
    }

    #[test]
    fn paid_without_price_is_distinct_from_unknown() {
        // §27: "paid, price not currently known" must NOT collapse into Unknown.
        let paid_no_price = Cost::from_kind_and_price("paid", None).unwrap();
        assert_eq!(paid_no_price, Cost::Paid { price: None });
        assert_ne!(paid_no_price, Cost::Unknown);
        assert_eq!(paid_no_price.kind_code(), "paid");
    }

    #[test]
    fn cost_from_kind_rejects_unknown_kind() {
        let err = Cost::from_kind_and_price("gratis", None).unwrap_err();
        assert!(matches!(err, DomainError::Invalid(m) if m.contains("gratis")));
    }

    // --- ParkingType (§26) -----------------------------------------------

    #[test]
    fn parking_type_codes_round_trip_for_every_variant() {
        for t in ParkingType::ALL {
            assert_eq!(ParkingType::from_code(t.as_code()).unwrap(), *t);
        }
    }

    #[test]
    fn unknown_parking_type_is_rejected_not_mapped_to_other() {
        let err = ParkingType::from_code("flying_carpet").unwrap_err();
        assert!(matches!(err, DomainError::Invalid(m) if m.contains("flying_carpet")));
    }

    // --- CurrencyCode (§27) ----------------------------------------------

    #[test]
    fn currency_code_accepts_three_letters_and_uppercases() {
        assert_eq!(CurrencyCode::parse("brl").unwrap().as_str(), "BRL");
        assert_eq!(CurrencyCode::parse("  usd ").unwrap().as_str(), "USD");
    }

    #[test]
    fn currency_code_rejects_bad_length_or_non_alpha() {
        assert!(CurrencyCode::parse("BR").is_err());
        assert!(CurrencyCode::parse("REAL").is_err());
        assert!(CurrencyCode::parse("R$1").is_err());
        assert!(CurrencyCode::parse("").is_err());
    }

    // --- PricingUnit (§27) -----------------------------------------------

    #[test]
    fn pricing_unit_round_trips_and_rejects_unknown() {
        for u in [
            PricingUnit::Hour,
            PricingUnit::Day,
            PricingUnit::Month,
            PricingUnit::Entry,
        ] {
            assert_eq!(PricingUnit::from_code(u.as_code()).unwrap(), u);
        }
        assert!(PricingUnit::from_code("year").is_err());
    }
}
