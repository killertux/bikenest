//! Search use cases: `SearchParking` (§21/§31–§34) and `GetParkingDetails` (§24).

use crate::ports::{
    FreshnessConfig, GeoHit, GeocodeError, Geocoder, ParkingDetailsReader, ParkingSearchReader,
    ReaderError, SearchInput, SearchPage, SearchRequest,
};
use bikenest_domain::{FreshnessCategory, GeoPoint};

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    /// No destination: no coordinates given and the query could not be geocoded.
    #[error("no destination given or resolvable")]
    MissingDestination,
    #[error("origin coordinates are invalid")]
    InvalidOrigin,
    #[error(transparent)]
    Geocode(#[from] GeocodeError),
    #[error(transparent)]
    Read(#[from] ReaderError),
}

/// Recommendation weights (§34). Hardcoded defaults for M1 — **Ledger #8**
/// (make configurable in M7). All sub-scores are 0..1, missing input → 0.5
/// (§34: missing information is never automatically the worst value).
#[derive(Debug, Clone, Copy)]
pub struct RecommendationConfig {
    pub w_distance: f64,
    pub w_security: f64,
    pub w_rating: f64,
    pub w_freshness: f64,
    pub w_verification: f64,
    /// Fetch cap for candidates scored in memory (see ports).
    pub candidate_cap: usize,
}

pub const DEFAULT_RECOMMENDATION_CONFIG: RecommendationConfig = RecommendationConfig {
    w_distance: 0.35,
    w_security: 0.25,
    w_rating: 0.20,
    w_freshness: 0.15,
    w_verification: 0.05,
    candidate_cap: 500,
};

/// Deterministic recommendation score for one summary row (§34).
/// Same input → same output; ties broken by (score DESC, id ASC).
pub fn recommendation_score(
    item: &crate::ports::ParkingSummary,
    radius_m: u32,
    now: chrono::DateTime<chrono::Utc>,
    weights: &RecommendationConfig,
    freshness: &FreshnessConfig,
) -> f64 {
    // Distance: closer is better within the search radius.
    let d = (item.distance_m / radius_m as f64).clamp(0.0, 1.0);
    let distance_score = 1.0 - d;

    // Security: share of the initial catalog (§28) explicitly confirmed.
    // No confirmed attributes → neutral (unknown is not "bad").
    let yes = item.security_yes.len() as f64;
    let security_score = if yes > 0.0 { (yes / 8.0).min(1.0) } else { 0.5 };

    // Rating: normalized 0..5; no reviews → neutral.
    let rating_score = item.rating.avg().map(|a| a / 5.0).unwrap_or(0.5);

    // Freshness: category → monotone score; never verified → neutral.
    let category = bikenest_domain::categorize(item.last_verified_at, now, &freshness.thresholds);
    let freshness_score = match category {
        FreshnessCategory::Fresh => 1.0,
        FreshnessCategory::RecentlyVerified => 0.75,
        FreshnessCategory::Aging => 0.5,
        FreshnessCategory::Stale => 0.25,
        FreshnessCategory::VeryStale => 0.1,
        FreshnessCategory::Never => 0.5,
    };

    // Verification confidence: ever verified → 1.0; never → neutral (§34).
    let verification_score = if item.last_verified_at.is_some() {
        1.0
    } else {
        0.5
    };

    weights.w_distance * distance_score
        + weights.w_security * security_score
        + weights.w_rating * rating_score
        + weights.w_freshness * freshness_score
        + weights.w_verification * verification_score
}

/// Use case: search parking near a destination (§21, §31–§34).
pub struct SearchParking {
    geocoder: Box<dyn Geocoder>,
    reader: Box<dyn ParkingSearchReader>,
    recommendation: RecommendationConfig,
    freshness: FreshnessConfig,
}

impl SearchParking {
    pub fn new(
        geocoder: Box<dyn Geocoder>,
        reader: Box<dyn ParkingSearchReader>,
        recommendation: RecommendationConfig,
        freshness: FreshnessConfig,
    ) -> Self {
        Self {
            geocoder,
            reader,
            recommendation,
            freshness,
        }
    }

    /// Executes the search. Returns the result page plus the geocode hit when
    /// the origin came from resolving a query (the web layer shows the
    /// resolved label; the coordinates are never persisted — §22).
    pub async fn execute(
        &self,
        input: SearchInput,
    ) -> Result<(SearchPage, Option<GeoHit>), SearchError> {
        let (request, geohit) = self.resolve(input).await?;
        let page = if request.sort == crate::ports::Sort::Recommended {
            self.recommended_page(&request).await?
        } else {
            self.sql_page(&request).await?
        };
        Ok((page, geohit))
    }

    /// Origin resolution (§21/§22): explicit coordinates win over the query;
    /// otherwise geocode the query. Nothing is persisted here.
    async fn resolve(
        &self,
        input: SearchInput,
    ) -> Result<(SearchRequest, Option<GeoHit>), SearchError> {
        let query = input
            .query
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty());

        let origin = if let (Some(lat), Some(lon)) = (input.lat, input.lon) {
            let point = GeoPoint::new(lat, lon).map_err(|_| SearchError::InvalidOrigin)?;
            (point, None)
        } else {
            let Some(q) = query else {
                return Err(SearchError::MissingDestination);
            };
            let Some(hit) = self.geocoder.geocode(q).await? else {
                return Err(SearchError::MissingDestination);
            };
            (hit.point, Some(hit))
        };
        let (point, geohit) = origin;

        let request = SearchRequest::new(
            point,
            geohit
                .as_ref()
                .map(|h| h.label.clone())
                .or_else(|| query.map(str::to_string)),
            input.radius_m.unwrap_or(crate::ports::DEFAULT_RADIUS_M),
            input.filters(),
            input
                .sort
                .as_deref()
                .and_then(crate::ports::Sort::from_code)
                .unwrap_or(crate::ports::Sort::Recommended),
            input.page_size.unwrap_or(crate::ports::DEFAULT_PAGE_SIZE),
            input.cursor.as_deref(),
        );
        Ok((request, geohit))
    }

    async fn sql_page(&self, request: &SearchRequest) -> Result<SearchPage, SearchError> {
        // Fetch one extra row to know whether a next page exists (§32).
        let mut page = self
            .reader
            .search(request, request.page_size + 1, true)
            .await?;
        let next_cursor = if page.items.len() > request.page_size {
            page.items.truncate(request.page_size);
            let last = page.items.last().expect("non-empty");
            Some(crate::ports::Cursor {
                sort: request.sort,
                v: last
                    .sort_key(request.sort)
                    .expect("SQL sorts always have a key"),
                id: last.id,
            })
        } else {
            None
        };
        Ok(SearchPage {
            total: page.total,
            next_cursor,
            items: page.items,
        })
    }

    /// Recommended sort: score a capped candidate set in memory (§34),
    /// deterministic ordering, ties by (score DESC, id ASC).
    async fn recommended_page(&self, request: &SearchRequest) -> Result<SearchPage, SearchError> {
        let candidates = self
            .reader
            .search(request, self.recommendation.candidate_cap, false)
            .await?;
        let now = chrono::Utc::now();
        let mut scored: Vec<(f64, crate::ports::ParkingSummary)> = candidates
            .items
            .into_iter()
            .map(|item| {
                let s = recommendation_score(
                    &item,
                    request.radius_m,
                    now,
                    &self.recommendation,
                    &self.freshness,
                );
                (s, item)
            })
            .collect();
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.1.id.cmp(&b.1.id))
        });

        // Advance past the cursor (score, id) if one is present.
        if let Some(c) = request.cursor {
            scored.retain(|(s, item)| *s > c.v || (*s == c.v && item.id > c.id));
        }

        let total = candidates.total;
        let has_more = scored.len() > request.page_size;
        let mut scored_items = scored;
        scored_items.truncate(request.page_size);
        let next_cursor = if has_more {
            scored_items
                .last()
                .map(|(score, last)| crate::ports::Cursor {
                    sort: request.sort,
                    v: *score,
                    id: last.id,
                })
        } else {
            None
        };
        let items: Vec<crate::ports::ParkingSummary> =
            scored_items.into_iter().map(|(_, item)| item).collect();
        Ok(SearchPage {
            items,
            total,
            next_cursor,
        })
    }
}

/// Result of the details use case: full aggregate + computed display facts.
#[derive(Debug, Clone)]
pub struct ParkingDetailsView {
    pub location: bikenest_domain::ParkingLocation,
    pub freshness: FreshnessCategory,
    pub is_open_now: bikenest_domain::OpenStatus,
}

#[derive(Debug, thiserror::Error)]
pub enum DetailsError {
    #[error(transparent)]
    Read(#[from] ReaderError),
}

/// Use case: everything the P3 details page needs (§24).
pub struct GetParkingDetails {
    reader: Box<dyn ParkingDetailsReader>,
    freshness: FreshnessConfig,
}

impl GetParkingDetails {
    pub fn new(reader: Box<dyn ParkingDetailsReader>, freshness: FreshnessConfig) -> Self {
        Self { reader, freshness }
    }

    pub async fn execute(&self, id: i64) -> Result<Option<ParkingDetailsView>, DetailsError> {
        let Some(location) = self.reader.details(id).await? else {
            return Ok(None);
        };
        let now = chrono::Utc::now();
        let freshness = bikenest_domain::categorize(
            location.last_verified_at(),
            now,
            &self.freshness.thresholds,
        );
        let is_open_now = location.hours().status_at(now, location.timezone());
        Ok(Some(ParkingDetailsView {
            location,
            freshness,
            is_open_now,
        }))
    }
}
