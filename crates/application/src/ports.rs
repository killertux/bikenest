//! Ports and read models for parking search (REQUIREMENTS §9, §21, §31–§34).

use async_trait::async_trait;
use bikenest_domain::{Cost, FreshnessThresholds, GeoPoint, ParkingType, Rating};

/// A geocoding result for a free-text destination.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoHit {
    pub label: String,
    pub point: GeoPoint,
}

#[derive(Debug, thiserror::Error)]
pub enum GeocodeError {
    #[error("geocoder unavailable")]
    Unavailable,
    #[error("geocoder failed: {0}")]
    Unexpected(String),
}

/// Port: resolve a destination string to coordinates (§21).
#[async_trait]
pub trait Geocoder: Send + Sync {
    /// `Ok(None)` when nothing matches the query.
    async fn geocode(&self, query: &str) -> Result<Option<GeoHit>, GeocodeError>;
}

// ---------------------------------------------------------------------------
// Search criteria
// ---------------------------------------------------------------------------

/// Radius allowlist (§31). Default 1 km.
pub const RADIUS_OPTIONS_M: &[u32] = &[250, 500, 1000, 2000];
pub const DEFAULT_RADIUS_M: u32 = 1000;
/// Page size defaults/limits (§32).
pub const DEFAULT_PAGE_SIZE: usize = 20;
pub const MAX_PAGE_SIZE: usize = 100;
/// Hard cap on candidates fetched for the in-memory "recommended" sort
/// (plans/m1-search-map.md §2); the total count is unaffected (SQL window).
pub const RECOMMENDED_CANDIDATE_CAP: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Recommended,
    Distance,
    Security,
    Rating,
    RecentlyVerified,
}

impl Sort {
    pub fn as_code(&self) -> &'static str {
        match self {
            Sort::Recommended => "recommended",
            Sort::Distance => "distance",
            Sort::Security => "security",
            Sort::Rating => "rating",
            Sort::RecentlyVerified => "recently_verified",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "recommended" => Some(Sort::Recommended),
            "distance" => Some(Sort::Distance),
            "security" => Some(Sort::Security),
            "rating" => Some(Sort::Rating),
            "recently_verified" => Some(Sort::RecentlyVerified),
            _ => None,
        }
    }
}

/// Cost filter (§33). `Paid` matches any paid location regardless of price.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostFilter {
    Free,
    Paid,
    Unknown,
}

impl CostFilter {
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "free" => Some(CostFilter::Free),
            "paid" => Some(CostFilter::Paid),
            "unknown" => Some(CostFilter::Unknown),
            _ => None,
        }
    }

    /// Does a location's cost match this filter?
    pub fn matches(&self, cost: &Cost) -> bool {
        matches!(
            (self, cost),
            (CostFilter::Free, Cost::Free)
                | (CostFilter::Paid, Cost::Paid { .. })
                | (CostFilter::Unknown, Cost::Unknown)
        )
    }
}

/// Structured filters (§33). All optional; all ANDed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filters {
    pub cost: Option<CostFilter>,
    pub types: Vec<ParkingType>,
    /// Location must have ALL of these security attributes confirmed (state=yes).
    pub security_all: Vec<String>,
    pub open_now: bool,
}

/// Opaque keyset cursor: `(sort_key, id)`, base64-encoded JSON (§32).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cursor {
    pub sort: Sort,
    /// Normalized ascending sort key (see infrastructure query: distance keeps
    /// its value; other sorts are negated so every sort paginates ascending).
    pub v: f64,
    pub id: i64,
}

impl Cursor {
    pub fn encode(&self) -> String {
        let json = serde_json::json!({ "s": self.sort.as_code(), "v": self.v, "id": self.id });
        use base64::Engine as _;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json.to_string())
    }

    /// Decodes a cursor; unknown/invalid cursors fall back to page 1 rather
    /// than erroring (defensive against tampered URLs).
    pub fn decode(raw: &str) -> Option<Self> {
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(raw)
            .ok()?;
        let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        let sort = Sort::from_code(json.get("s")?.as_str()?)?;
        let v = json.get("v")?.as_f64()?;
        let id = json.get("id")?.as_i64()?;
        Some(Self { sort, v, id })
    }
}

/// Raw, untrusted search input from the web layer (§7: handlers only map
/// HTTP params; business rules live here).
#[derive(Debug, Clone, Default)]
pub struct SearchInput {
    /// Free-text destination (§21). Ignored when explicit coordinates exist.
    pub query: Option<String>,
    /// Explicit origin (browser geolocation §22, or bookmarked URL).
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub radius_m: Option<u32>,
    pub cost: Option<String>,
    /// Comma-separated type codes.
    pub types: Option<String>,
    /// Comma-separated security feature codes (all-of).
    pub security: Option<String>,
    pub open_now: bool,
    pub sort: Option<String>,
    pub page_size: Option<usize>,
    pub cursor: Option<String>,
}

impl SearchInput {
    /// Parses filters and options; coordinate/geometry validation happens in
    /// `SearchRequest::new`. Unknown codes are dropped (not errors) so a
    /// stale shared URL still renders results.
    pub fn filters(&self) -> Filters {
        Filters {
            cost: self.cost.as_deref().and_then(CostFilter::from_code),
            types: self
                .types
                .as_deref()
                .unwrap_or("")
                .split(',')
                .filter_map(|c| ParkingType::from_code(c).ok())
                .collect(),
            security_all: self
                .security
                .as_deref()
                .unwrap_or("")
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
            open_now: self.open_now,
        }
    }
}

/// Fully validated search request. Constructed only via [`SearchRequest::resolve`]
/// so business rules (radius allowlist, page-size clamp, filter sanity) cannot
/// be bypassed by the web layer.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchRequest {
    pub origin: GeoPoint,
    /// Resolved destination label, when the origin came from a geocode hit.
    pub destination_label: Option<String>,
    pub radius_m: u32,
    pub filters: Filters,
    pub sort: Sort,
    pub page_size: usize,
    pub cursor: Option<Cursor>,
}

impl SearchRequest {
    /// Validates and clamps: radius to the allowlist, page size to §32 limits,
    /// type codes, cursor/sort agreement (mismatched cursor → page 1).
    pub fn new(
        origin: GeoPoint,
        destination_label: Option<String>,
        radius_m: u32,
        filters: Filters,
        sort: Sort,
        page_size: usize,
        cursor_raw: Option<&str>,
    ) -> Self {
        let radius_m = if RADIUS_OPTIONS_M.contains(&radius_m) {
            radius_m
        } else {
            DEFAULT_RADIUS_M
        };
        let page_size = page_size.clamp(1, MAX_PAGE_SIZE);
        let cursor = cursor_raw
            .and_then(Cursor::decode)
            .filter(|c| c.sort == sort);
        // Deduplicate + validate type codes; drop unknown security codes.
        let mut types = filters.types.clone();
        types.sort_by_key(|t| t.as_code());
        types.dedup();
        let mut security_all = filters.security_all.clone();
        security_all.retain(|c| !c.trim().is_empty());
        security_all.sort();
        security_all.dedup();
        Self {
            origin,
            destination_label,
            radius_m,
            filters: Filters {
                cost: filters.cost,
                types,
                security_all,
                open_now: filters.open_now,
            },
            sort,
            page_size,
            cursor,
        }
    }
}

/// One row of search results — everything a P2 card needs.
#[derive(Debug, Clone, PartialEq)]
pub struct ParkingSummary {
    pub id: i64,
    pub name: String,
    pub address: String,
    pub parking_type: ParkingType,
    pub cost: Cost,
    pub point: GeoPoint,
    /// Distance from the search origin, meters (§31).
    pub distance_m: f64,
    /// Codes of security attributes explicitly confirmed present.
    pub security_yes: Vec<String>,
    pub rating: Rating,
    pub last_verified_at: Option<chrono::DateTime<chrono::Utc>>,
    pub timezone: chrono_tz::Tz,
    /// Computed in SQL from wall-clock hours (§29/§33).
    pub is_open_now: bool,
    /// Object-storage key of the location's primary approved photo, if any.
    /// The web layer resolves this to a presigned URL for the card; `None`
    /// falls back to a positional illustrative image.
    pub photo_key: Option<String>,
}

impl ParkingSummary {
    /// Normalized ascending sort key for SQL keyset pagination (§32).
    /// distance → itself; the other sorts negate so ascending works for all.
    /// The `Recommended` sort paginates on its own score, computed by the
    /// application layer, so it has no SQL-side key.
    pub fn sort_key(&self, sort: Sort) -> Option<f64> {
        match sort {
            Sort::Recommended => None,
            Sort::Distance => Some(self.distance_m),
            Sort::Security => Some(-(self.security_yes.len() as f64)),
            Sort::Rating => Some(-self.rating.avg().unwrap_or(2.5)),
            Sort::RecentlyVerified => Some(
                -self
                    .last_verified_at
                    .map(|t| t.timestamp_millis() as f64)
                    .unwrap_or(0.0),
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchPage {
    pub items: Vec<ParkingSummary>,
    /// Total matching the criteria within the radius (before pagination).
    pub total: i64,
    /// Present when another page exists.
    pub next_cursor: Option<Cursor>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReaderError {
    #[error("database unavailable")]
    Unavailable,
    #[error("database error: {0}")]
    Unexpected(String),
}

/// Port: keyset-paginated nearby search over `parking_location` (§31–§32).
#[async_trait]
pub trait ParkingSearchReader: Send + Sync {
    /// Applies criteria (except `Recommended` sorting, which the use case
    /// performs in the application layer over a capped candidate set).
    /// Ignores `cursor` unless `cursor.sort == Sort::Distance` etc. — i.e.
    /// applies it when it matches the SQL sort; the use case coordinates this.
    async fn search(
        &self,
        request: &SearchRequest,
        limit: usize,
        apply_cursor: bool,
    ) -> Result<SearchPage, ReaderError>;
}

/// Port: full aggregate for the details page (§24).
#[async_trait]
pub trait ParkingDetailsReader: Send + Sync {
    async fn details(
        &self,
        id: i64,
    ) -> Result<Option<bikenest_domain::ParkingLocation>, ReaderError>;
}

/// A stored photo reference: an opaque object-storage key plus its content type
/// and accessible description. The web layer turns the key into a presigned URL.
/// When a processed thumbnail exists (M4 uploads), [`Self::thumbnail_key`] is
/// present and the gallery prefers it; seeded M1 photos fall back to [`Self::key`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPhoto {
    pub key: String,
    pub thumbnail_key: Option<String>,
    pub content_type: String,
    pub alt: Option<String>,
}

/// Port: approved photos for a location, in display order (P3 gallery / P2
/// card). Kept separate from [`ParkingDetailsReader`] so the details aggregate
/// and its use case stay unchanged (photos are a read-side, presentation
/// concern joined at the web boundary).
#[async_trait]
pub trait ParkingPhotoReader: Send + Sync {
    async fn photos(&self, location_id: i64) -> Result<Vec<StoredPhoto>, ReaderError>;
}

/// Port: approved photos attached to a *review* (D3 §38), in display order.
/// Only `APPROVED` review photos render on the review card.
#[async_trait]
pub trait ReviewPhotosReader: Send + Sync {
    async fn photos(&self, review_id: i64) -> Result<Vec<StoredPhoto>, ReaderError>;
}

/// Shared freshness configuration for view-building use cases.
#[derive(Debug, Clone, Copy)]
pub struct FreshnessConfig {
    pub thresholds: FreshnessThresholds,
}

impl Default for FreshnessConfig {
    fn default() -> Self {
        Self {
            thresholds: bikenest_domain::DEFAULT_THRESHOLDS,
        }
    }
}
