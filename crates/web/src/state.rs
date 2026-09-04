//! The shared state every handler reads from.
//!
//! One value, cloned per request: services, read-side ports and the
//! configuration-derived settings (map, security policy, base URL, asset
//! manifest). It is built once in [`crate::wiring`] — the only module that
//! knows which concrete provider backs each port.

use std::sync::Arc;

use bikenest_application::{
    AuthService, CheckReadiness, ContributionService, FreshnessConfig, GetParkingDetails,
    ModerationService, ObjectStorage, ParkingPhotoReader, PhotoService, PrivacyService,
    RateLimiter, SearchParking, SitemapReader,
};
use bikenest_infrastructure::probe::SqlxDatabaseProbe;
use bikenest_infrastructure::{CachingGeocoder, Config, GeocodeLimits, MapConfig};

use crate::security::SecurityHeaders;

/// Shared application state wired at startup. Everything configuration-derived
/// is resolved once here — no handler reads the process environment.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub readiness: Arc<CheckReadiness<SqlxDatabaseProbe>>,
    pub search: Arc<SearchParking>,
    /// The very geocoder the search use case calls, so `/search` can ask
    /// whether a destination is already resolved before spending any of the
    /// caller's geocode budget on it.
    pub geocoder: Arc<CachingGeocoder>,
    /// Shared limiter store, for the per-IP geocode budget.
    pub rate_limiter: Arc<dyn RateLimiter>,
    /// That budget's size and window.
    pub geocode_limits: GeocodeLimits,
    pub details: Arc<GetParkingDetails>,
    /* Configured freshness thresholds, used for display categorization so the
    cards honour the same tunable value as the search/detail services. */
    pub freshness: FreshnessConfig,
    pub photos: Arc<dyn ParkingPhotoReader>,
    /// The ids `/sitemap.xml` lists. A read-side port of its own, so the one
    /// page that needs "every public location" does not reach for the pool.
    pub sitemap: Arc<dyn SitemapReader>,
    pub storage: Arc<dyn ObjectStorage>,
    pub auth: Arc<AuthService>,
    pub contributions: Arc<ContributionService>,
    pub photo: Arc<PhotoService>,
    pub moderation: Arc<ModerationService>,
    pub privacy: Arc<PrivacyService>,
    pub policy: Arc<dyn bikenest_application::PolicyReader>,
    /// Security/CSP header policy, built once from the configured origins.
    pub security: SecurityHeaders,
    /// Client-side map style/token rendered into every page layout.
    pub map: MapConfig,
    /// Public origin absolute links are built from (canonical URLs, sitemap).
    pub base_url: String,
    /// Google sign-in feature flag (disabled until a real OAuth provider exists).
    pub google_oauth_enabled: bool,
    /// Content-hash manifest for `/static/...` (WP14): logical path → hash,
    /// computed once at startup by `crate::assets::init`. Backs the
    /// `/static/h/{hash}/{*path}` handler; `PageLayout::asset()` resolves
    /// URLs from the same manifest.
    pub assets: crate::assets::AssetManifest,
}
