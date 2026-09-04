//! Search use cases: `SearchParking` and `GetParkingDetails`.

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

/// Recommendation weights. All sub-scores are 0..1, and missing input scores
/// a neutral 0.5: absent information is never automatically the worst value.
///
/// The score is computed by the search reader, as that query's sort key, so
/// these weights are bound into the SQL rather than applied in Rust: a score
/// assembled after a page is read can only rank the rows that page happened
/// to contain.
#[derive(Debug, Clone, Copy)]
pub struct RecommendationConfig {
    pub w_distance: f64,
    pub w_security: f64,
    pub w_rating: f64,
    pub w_freshness: f64,
    pub w_verification: f64,
}

pub const DEFAULT_RECOMMENDATION_CONFIG: RecommendationConfig = RecommendationConfig {
    w_distance: 0.35,
    w_security: 0.25,
    w_rating: 0.20,
    w_freshness: 0.15,
    w_verification: 0.05,
};

/// Use case: search parking near a destination.
pub struct SearchParking {
    geocoder: Box<dyn Geocoder>,
    reader: Box<dyn ParkingSearchReader>,
}

impl SearchParking {
    /// The reader carries the recommendation weights and freshness thresholds
    /// (it computes the sort key), so this use case holds no scoring config of
    /// its own: origin resolution and cursor handling are all it decides.
    pub fn new(geocoder: Box<dyn Geocoder>, reader: Box<dyn ParkingSearchReader>) -> Self {
        Self { geocoder, reader }
    }

    /// Executes the search. Returns the result page plus the geocode hit when
    /// the origin came from resolving a query (the web layer shows the
    /// resolved label; the coordinates are never persisted — §22).
    pub async fn execute(
        &self,
        input: SearchInput,
    ) -> Result<(SearchPage, Option<GeoHit>), SearchError> {
        let (request, geohit) = self.resolve(input).await?;
        let page = self.page(&request).await?;
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

    /// One page, for every sort: the reader orders and limits, this only turns
    /// the last row's key into the next cursor.
    async fn page(&self, request: &SearchRequest) -> Result<SearchPage, SearchError> {
        // Fetch one extra row to know whether a next page exists (§32).
        let mut page = self
            .reader
            .search(request, request.page_size + 1, true)
            .await?;
        let next_cursor = if page.items.len() > request.page_size {
            page.items.truncate(request.page_size);
            let last = page.items.last().expect("non-empty");
            // The anchor is the key the reader itself computed for this row;
            // recomputing it here would risk disagreeing with the SQL keyset
            // predicate and paginating in circles.
            Some(crate::ports::Cursor {
                sort: request.sort,
                v: last.sort_key.expect("SQL sorts always carry a key"),
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
