//! HTTP routing and handlers.

use askama::Template;
use axum::extract::{Form, Multipart};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Json, Redirect, Response};
use axum::routing::{get, post};
use axum::{
    Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    middleware,
};
use bikenest_application::{
    AuditFilter, AuthError, AuthService, CheckReadiness, ContributionDeps, ContributionError,
    ContributionService, EmailProvider, FreshnessConfig, GetParkingDetails, ModerationDeps,
    ModerationError, ModerationService, NewParkingLocation, NewVerification, ObjectStorage,
    POLICY_FALLBACK_LOCALE, ParkingEdit, ParkingPhotoReader, PasswordHasher, PhotoDeps, PhotoError,
    PhotoKind, PhotoService, PhotoTarget, PrivacyDeps, PrivacyError, PrivacyService,
    ProposalApplication, RateLimiter, Readiness, SearchInput, SearchParking, TokenGenerator,
};
use bikenest_domain::{
    Cost, CurrencyCode, GeoPoint, ModerationState, Money, OpeningHours, ParkingLocation,
    ParkingType, PolicyKind, PricingUnit, PrivacyRequestKind, ReportOutcome, ReportState,
    ReportTargetType, ReviewBody, SecurityFeature, SecurityState, StarRating, TimeRange,
    is_known_attribute_code, is_known_security_code,
};
use bikenest_domain::{ExistenceResult, ProposalKind, Role, UserEmail, UserId};
use bikenest_infrastructure::probe::SqlxDatabaseProbe;
use bikenest_infrastructure::{
    Argon2PasswordHasher, Config, ConfigError, Db, FakeOAuthProvider, LocalImageProcessor,
    MapConfig, OfflineTimezoneResolver, RealTokenGenerator, S3ObjectStorage, SharedObjectStorage,
    SharedRateLimiter, SqlxAccountRepository, SqlxAnonymizationRepository, SqlxAuditLog,
    SqlxAuditLogReader, SqlxContributionHistoryReader, SqlxExportRepository,
    SqlxFavoriteRepository, SqlxModerationRepository, SqlxParkingContributionRepository,
    SqlxParkingDetailsReader, SqlxParkingPhotoReader, SqlxParkingSearchReader, SqlxPhotoRepository,
    SqlxPolicyReader, SqlxPrivacyRequestRepository, SqlxReportRepository, SqlxReviewPhotosReader,
    SqlxReviewRepository, SqlxSessionStore, SqlxTokenStore, SqlxVerificationRepository,
    SystemClock, email_from_config, geocoder_from_config, rate_limiter_from_config,
};
use serde_json::json;
use std::sync::Arc;

use crate::auth::{
    Auth, anon_csrf_token, clear_session_cookie, set_anon_csrf_cookie, set_session_cookie,
};
use crate::client_ip::ClientIp;
use crate::htmx::{self, fragment_or_redirect, is_fragment_request, vary_fragment};
use crate::i18n::{Locale, Translator};
use crate::security::SecurityHeaders;
use crate::view::{self, CardVm, ResultsData};
use crate::{
    AboutPage, AccountDeletePage, AccountEmailPage, AccountExportPage, AccountPage,
    AccountPasswordPage, AccountPrivacyPage, AdminAuditPage, AdminPrivacyRequestsPage,
    AdminUserContributionsPage, AdminUsersPage, ContributionsPage, DetailsPage,
    FavoritesPage, HomePage, LoginPage, ModerationActionResultVm, ModerationDashboardPage,
    ModerationPhotosPage, ModerationProposalsPage, ModerationReportsPage, PageLayout,
    ParkingEditPage, ParkingNewPage, PasswordResetNewPage, PasswordResetPage, PhotoVm, PolicyPage,
    PolicyVersionsPage, RegisterPage, ReportResultVm, ReviewFormPage, SearchPageVm,
    SearchResultsVm, VerifyEmailPage,
};

/// Shared application state wired at startup. Everything configuration-derived
/// is resolved once here — no handler reads the process environment.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub readiness: Arc<CheckReadiness<SqlxDatabaseProbe>>,
    pub db: Db,
    pub search: Arc<SearchParking>,
    pub details: Arc<GetParkingDetails>,
    /* Configured freshness thresholds, used for display categorization so the
    cards honour the same tunable value as the search/detail services. */
    pub freshness: FreshnessConfig,
    pub photos: Arc<dyn ParkingPhotoReader>,
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
}

/// The doubles a test injects. Production passes [`RouterDeps::from_config`],
/// which builds every one of them from the parsed configuration.
pub struct RouterDeps<H: PasswordHasher + Clone + 'static> {
    pub email: Box<dyn EmailProvider>,
    /// `None` builds the deterministic stub from `Config::fake_oauth`.
    pub oauth: Option<FakeOAuthProvider>,
    pub hasher: H,
    pub rate_limiter: Box<dyn RateLimiter>,
    pub storage: Arc<dyn ObjectStorage>,
}

impl RouterDeps<Argon2PasswordHasher> {
    /// The real providers the configuration selected. Fails rather than
    /// substituting a fake when a provider the operator asked for cannot be
    /// built.
    pub fn from_config(config: &Config) -> Result<Self, ConfigError> {
        Ok(Self {
            email: email_from_config(&config.email)?,
            oauth: None,
            hasher: Argon2PasswordHasher,
            rate_limiter: rate_limiter_from_config(&config.rate_limiter)?,
            storage: Arc::new(S3ObjectStorage::from_config(&config.storage)),
        })
    }
}

/// Builds the full application router with the real providers the configuration
/// selected.
pub fn app_router(config: Arc<Config>, db: Db) -> Result<Router, ConfigError> {
    let deps = RouterDeps::from_config(&config)?;
    Ok(app_router_with(config, db, deps))
}

/// Builds the router from the parsed configuration plus injectable
/// email/OAuth/password/rate-limiter/storage providers (tests pass fakes — e.g.
/// a fast [`TestPasswordHasher`] and a fresh in-memory limiter — to keep the
/// suite fast and isolated).
///
/// The passed limiter is shared by the auth, contributions, photo and
/// moderation services so they all read one store.
pub fn app_router_with<H: PasswordHasher + Clone + 'static>(
    config: Arc<Config>,
    db: Db,
    deps: RouterDeps<H>,
) -> Router {
    let RouterDeps {
        email,
        oauth,
        hasher,
        rate_limiter,
        storage,
    } = deps;
    let oauth = oauth.unwrap_or_else(|| FakeOAuthProvider::from_config(&config.fake_oauth));
    let google_oauth_enabled = config.google_oauth_enabled;
    let rate_limiter: Arc<dyn RateLimiter> = Arc::from(rate_limiter);
    let probe = SqlxDatabaseProbe::new(db.clone(), config.probe_timeout);
    let search_uc = SearchParking::new(
        geocoder_from_config(&config.geocoder),
        Box::new(SqlxParkingSearchReader::new(db.clone())),
        config.recommendation,
        config.freshness,
    );
    let details = GetParkingDetails::new(
        Box::new(SqlxParkingDetailsReader::new(db.clone())),
        config.freshness,
    );
    let auth_service = AuthService::new(
        Box::new(SqlxAccountRepository::new(db.clone())),
        Box::new(SqlxSessionStore::new(db.clone())),
        Box::new(SqlxTokenStore::new(db.clone())),
        Box::new(hasher.clone()), // password hasher (Argon2 in prod, fast fake in tests)
        Box::new(RealTokenGenerator),
        Box::new(SystemClock),
        email,
        Box::new(oauth),
        Box::new(SharedRateLimiter::new(rate_limiter.clone())), // shared ValKey store
        Box::new(SqlxAuditLog::new(db.clone())),
        config.base_url.clone(),
    );
    let contribution_service = ContributionService::new(ContributionDeps {
        tz: Box::new(OfflineTimezoneResolver::new()),
        details: Box::new(SqlxParkingDetailsReader::new(db.clone())),
        contributions: Box::new(SqlxParkingContributionRepository::new(db.clone())),
        reviews: Box::new(SqlxReviewRepository::new(db.clone())),
        verifications: Box::new(SqlxVerificationRepository::new(db.clone())),
        favorites: Box::new(SqlxFavoriteRepository::new(db.clone())),
        history: Box::new(SqlxContributionHistoryReader::new(db.clone())),
        review_photos: Box::new(SqlxReviewPhotosReader::new(db.clone())),
        rate_limiter: Box::new(SharedRateLimiter::new(rate_limiter.clone())),
        audit: Box::new(SqlxAuditLog::new(db.clone())),
        clock: Box::new(SystemClock),
        freshness: config.freshness,
    });
    let photo_service = PhotoService::new(PhotoDeps {
        processor: Box::new(LocalImageProcessor::new(config.photo)),
        repository: Box::new(SqlxPhotoRepository::new(db.clone())),
        storage: Box::new(SharedObjectStorage::new(storage.clone())),
        rate_limiter: Box::new(SharedRateLimiter::new(rate_limiter.clone())),
        audit: Box::new(SqlxAuditLog::new(db.clone())),
        clock: Box::new(SystemClock),
        limits: config.photo,
    });
    let moderation_service = ModerationService::new(ModerationDeps {
        reports: Box::new(SqlxReportRepository::new(db.clone())),
        moderation: Box::new(SqlxModerationRepository::new(db.clone())),
        audit: Box::new(SqlxAuditLog::new(db.clone())),
        audit_reader: Box::new(SqlxAuditLogReader::new(db.clone())),
        history: Box::new(SqlxContributionHistoryReader::new(db.clone())),
        rate_limiter: Box::new(SharedRateLimiter::new(rate_limiter.clone())),
        limits: config.moderation,
    });
    let privacy_service = PrivacyService::new(PrivacyDeps {
        exports: Box::new(SqlxExportRepository::new(db.clone())),
        requests: Box::new(SqlxPrivacyRequestRepository::new(db.clone())),
        anonymization: Box::new(SqlxAnonymizationRepository::new(db.clone())),
        accounts: Box::new(SqlxAccountRepository::new(db.clone())),
        sessions: Box::new(SqlxSessionStore::new(db.clone())),
        audit: Box::new(SqlxAuditLog::new(db.clone())),
        hasher: Box::new(hasher),
        tokens_gen: Box::new(RealTokenGenerator),
        clock: Box::new(SystemClock),
    });
    let policy_reader: Arc<dyn bikenest_application::PolicyReader> =
        Arc::new(SqlxPolicyReader::new(db.clone()));
    let state = AppState {
        readiness: Arc::new(CheckReadiness::new(probe)),
        db: db.clone(),
        search: Arc::new(search_uc),
        details: Arc::new(details),
        freshness: config.freshness,
        photos: Arc::new(SqlxParkingPhotoReader::new(db.clone())),
        storage: storage.clone(),
        auth: Arc::new(auth_service),
        contributions: Arc::new(contribution_service),
        photo: Arc::new(photo_service),
        moderation: Arc::new(moderation_service),
        privacy: Arc::new(privacy_service),
        policy: policy_reader,
        security: SecurityHeaders::new(&config.security, config.tls_on),
        map: config.map.clone(),
        base_url: config.base_url.clone(),
        google_oauth_enabled,
        config: config.clone(),
    };
    let mut router = Router::new()
        .route("/", get(home))
        .route("/search", get(search))
        .route("/parking/{id}", get(parking_details))
        .route("/about", get(about))
        .route("/robots.txt", get(robots_txt))
        .route("/sitemap.xml", get(sitemap_xml))
        .route("/lang/{code}", get(set_lang))
        .route("/media/{*key}", get(media))
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        // --- Accounts & authentication (M2) ---
        .route("/register", get(register_page).post(register_post))
        .route("/login", get(login_page).post(login_post))
        .route("/logout", post(logout))
        .route("/verify-email", get(verify_email))
        .route("/verify-email/resend", post(verify_resend))
        .route(
            "/password-reset",
            get(password_reset_page).post(password_reset_post),
        )
        .route(
            "/password-reset/new",
            get(password_reset_new).post(password_reset_new_post),
        );
    // Google sign-in (product decision: disabled until a real OAuth provider
    // exists). Unregistered routes fall through to the styled 404 handler.
    if google_oauth_enabled {
        router = router
            .route("/auth/google", get(auth_google))
            .route("/auth/google/fake-consent", get(auth_google_fake_consent))
            .route("/auth/google/callback", get(auth_google_callback));
    }
    router
        .route("/account", get(account))
        .route(
            "/account/password",
            get(account_password).post(account_password_post),
        )
        .route(
            "/account/email",
            get(account_email).post(account_email_post),
        )
        // --- M6 privacy & account lifecycle ---
        .route("/privacy", get(privacy_page))
        .route("/terms", get(terms_page))
        .route("/cookies", get(cookies_page))
        .route("/privacy/versions", get(privacy_versions))
        .route("/terms/versions", get(terms_versions))
        .route("/cookies/versions", get(cookies_versions))
        .route("/account/privacy", get(account_privacy))
        .route("/account/privacy/export", post(account_export_post))
        .route(
            "/account/privacy/request",
            post(account_privacy_request_post),
        )
        .route("/account/export/{id}", get(account_export))
        .route(
            "/account/export/{id}/download",
            get(account_export_download),
        )
        .route(
            "/account/delete",
            get(account_delete).post(account_delete_post),
        )
        .route("/admin/privacy-requests", get(admin_privacy_requests))
        .route(
            "/admin/privacy-requests/{id}/fulfill",
            post(admin_privacy_request_fulfill),
        )
        // --- M3 community contributions ---
        .route("/parking/new", get(parking_new_page).post(parking_new_post))
        .route(
            "/parking/{id}/edit",
            get(parking_edit_page).post(parking_edit_post),
        )
        .route("/parking/{id}/proposal", post(parking_proposal_post))
        .route("/parking/{id}/review", get(review_page).post(review_post))
        .route("/parking/{id}/verify", post(parking_verify_post))
        .route("/parking/{id}/parked-here", post(parking_parked_here_post))
        .route("/parking/{id}/favorite", post(parking_favorite_post))
        .route("/account/favorites", get(account_favorites))
        .route("/account/contributions", get(account_contributions))
        .route("/admin/users", get(admin_users))
        .route("/admin/users/{id}/role", post(admin_role_post))
        // --- M4 photos (upload → moderate → publish) ---
        .route(
            "/parking/{id}/photo",
            post(upload_photo).layer(DefaultBodyLimit::max(config.photo.max_bytes + 64 * 1024)),
        )
        .route("/moderation/photos", get(moderation_photos))
        .route(
            "/moderation/photos/{kind}/{id}/approve",
            post(moderation_photo_approve),
        )
        .route(
            "/moderation/photos/{kind}/{id}/reject",
            post(moderation_photo_reject),
        )
        .route(
            "/moderation/photos/{kind}/{id}/hide",
            post(moderation_photo_hide),
        )
        .route(
            "/moderation/photos/{kind}/{id}/restore",
            post(moderation_photo_restore),
        )
        // --- M5 reports + moderation actions + audit viewer ---
        .route("/reports", post(report_submit))
        .route("/moderation", get(moderation_dashboard))
        .route("/moderation/reports", get(moderation_reports))
        .route(
            "/moderation/reports/{id}/claim",
            post(moderation_report_claim),
        )
        .route(
            "/moderation/reports/{id}/resolve",
            post(moderation_report_resolve),
        )
        .route(
            "/moderation/reports/{id}/dismiss",
            post(moderation_report_dismiss),
        )
        .route("/moderation/proposals", get(moderation_proposals))
        .route(
            "/moderation/proposals/{id}/approve",
            post(moderation_proposal_approve),
        )
        .route(
            "/moderation/proposals/{id}/reject",
            post(moderation_proposal_reject),
        )
        .route(
            "/moderation/reviews/{id}/hide",
            post(moderation_review_hide),
        )
        .route(
            "/moderation/reviews/{id}/restore",
            post(moderation_review_restore),
        )
        .route(
            "/moderation/parking/{id}/invalidate",
            post(moderation_parking_invalidate),
        )
        .route(
            "/moderation/parking/{id}/restore",
            post(moderation_parking_restore),
        )
        .route("/admin/users/{id}/suspend", post(admin_user_suspend))
        .route("/admin/users/{id}/restore", post(admin_user_restore))
        .route(
            "/admin/users/{id}/contributions",
            get(admin_user_contributions),
        )
        .route("/admin/audit", get(admin_audit))
        // Served from `STATIC_ROOT` (resolved at startup), so the binary is
        // relocatable instead of pinned to its compile-time manifest path.
        .nest_service(
            "/static",
            tower_http::services::ServeDir::new(&config.static_root),
        )
        .fallback(not_found)
        // Order matters: the LAST `.layer()` is the OUTERMOST middleware. We want
        // the security-header middleware to wrap `auth_middleware` so that even
        // when auth short-circuits (e.g. a CSRF-403 without calling the inner
        // handler) the security/CSP headers are still applied. TraceLayer stays
        // outermost so it observes every response.
        //
        // `styled_errors` is innermost: it upgrades the plain-text failures the
        // router and axum's own extractors emit (a 405, a malformed multipart
        // boundary, a body over `DefaultBodyLimit`) into the styled page — or,
        // for a real htmx fragment request, into a swap-safe fragment.
        .layer(middleware::from_fn_with_state(state.clone(), styled_errors))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.security.clone(),
            crate::security::security_headers,
        ))
        .layer(
            tower_http::trace::TraceLayer::new_for_http()
                .make_span_with(crate::observability::RequestSpan)
                .on_response(crate::observability::RequestLog),
        )
        .with_state(state)
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz(State(state): State<AppState>) -> Response {
    match state.readiness.execute().await {
        Readiness::Ready => (
            StatusCode::OK,
            Json(json!({"status": "ready", "database": "up"})),
        )
            .into_response(),
        Readiness::DependencyDown => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "degraded", "database": "down"})),
        )
            .into_response(),
        Readiness::AppError => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"status": "error"})), // no internal details (§85)
        )
            .into_response(),
    }
}

/// GET /robots.txt — indexing policy (§109). Public pages are crawlable;
/// private account/admin/moderation paths are disallowed here (and also get
/// `X-Robots-Tag: noindex` on their responses).
async fn robots_txt() -> Response {
    const BODY: &str =
        "User-agent: *\nAllow: /\nDisallow: /account\nDisallow: /admin\nDisallow: /moderation\n";
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], BODY).into_response()
}

/// GET /sitemap.xml — static pages + ACTIVE public parking (§111 stable URLs).
async fn sitemap_xml(State(state): State<AppState>) -> Response {
    let base = state.base_url.clone();
    let static_urls = ["/", "/search", "/about", "/privacy", "/terms", "/cookies"];
    let parking_ids: Vec<i64> = bikenest_infrastructure::parking::active_parking_ids(&state.db)
        .await
        .unwrap_or_default();

    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for url in static_urls {
        xml.push_str(&format!("  <url><loc>{base}{url}</loc></url>\n"));
    }
    for id in parking_ids {
        xml.push_str(&format!("  <url><loc>{base}/parking/{id}</loc></url>\n"));
    }
    xml.push_str("</urlset>\n");
    (
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        xml,
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

/// P1 — home / landing.
async fn home(State(state): State<AppState>, locale: Locale, auth: Auth) -> Response {
    let tr = Translator::new(locale);
    // A few example locations near the featured landmark, when data exists
    // (P1: optional section). Failure → render without them.
    let mut featured = Vec::new();
    if let Ok((page, _)) = state
        .search
        .execute(SearchInput {
            query: Some("Rua XV de Novembro".to_string()),
            radius_m: Some(1000),
            page_size: Some(4),
            ..Default::default()
        })
        .await
    {
        let now = chrono::Utc::now();
        for s in &page.items {
            let photo_url = view::resolve_photo(&*state.storage, s.photo_key.as_deref()).await;
            featured.push(CardVm::from_summary(
                tr,
                s,
                bikenest_domain::categorize(s.last_verified_at, now, &state.freshness.thresholds),
                photo_url,
            ));
        }
    }

    let page = HomePage {
        layout: PageLayout::new(&state.map, tr.t("home.title").to_string(), "home")
            .canonical(format!("{}/", state.base_url.trim_end_matches('/')))
            .description(tr.t("home.hero.subtitle").to_string())
            .csrf(auth.csrf_value()),
        tr,
        featured,
    };
    render(page, StatusCode::OK)
}

/// Query parameters of `/search` (P2). Only mapping — validation and
/// business rules live in the application layer (§7).
#[derive(Debug, Default, Clone, serde::Deserialize)]
pub struct SearchParams {
    /// Plain `String` with a default: Askama templates cannot destructure
    /// `Option<String>` directly, so string params always exist (empty = unset).
    #[serde(default)]
    pub q: String,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub radius: Option<u32>,
    #[serde(default)]
    pub cost: String,
    /// `type` URL parameter (renamed: `type` is a Rust keyword).
    #[serde(rename = "type", default)]
    pub parking_type: String,
    #[serde(default)]
    pub security: String,
    #[serde(default)]
    pub open_now: String,
    #[serde(default)]
    pub sort: String,
    pub cursor: Option<String>,
}

impl SearchParams {
    fn to_input(&self) -> SearchInput {
        SearchInput {
            query: (!self.q.is_empty()).then(|| self.q.clone()),
            lat: self.lat,
            lon: self.lon,
            radius_m: self.radius,
            cost: (!self.cost.is_empty()).then(|| self.cost.clone()),
            types: (!self.parking_type.is_empty()).then(|| self.parking_type.clone()),
            security: (!self.security.is_empty()).then(|| self.security.clone()),
            open_now: self.open_now == "true",
            sort: (!self.sort.is_empty()).then(|| self.sort.clone()),
            page_size: None,
            cursor: self.cursor.clone(),
        }
    }

    /// Query string without the cursor (for building the next-page link).
    fn query_string(&self) -> String {
        let mut parts = Vec::new();
        if !self.q.is_empty() {
            parts.push(format!("q={}", urlencode(&self.q)));
        }
        if let Some(lat) = self.lat {
            parts.push(format!("lat={lat}"));
        }
        if let Some(lon) = self.lon {
            parts.push(format!("lon={lon}"));
        }
        if let Some(r) = self.radius {
            parts.push(format!("radius={r}"));
        }
        if !self.cost.is_empty() {
            parts.push(format!("cost={}", urlencode(&self.cost)));
        }
        if !self.parking_type.is_empty() {
            parts.push(format!("type={}", urlencode(&self.parking_type)));
        }
        if !self.security.is_empty() {
            parts.push(format!("security={}", urlencode(&self.security)));
        }
        if self.open_now == "true" {
            parts.push("open_now=true".to_string());
        }
        if !self.sort.is_empty() {
            parts.push(format!("sort={}", urlencode(&self.sort)));
        }
        parts.join("&")
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// P2 — search results (full page, or HTMX fragment when requested).
async fn search(
    State(state): State<AppState>,
    locale: Locale,
    headers: HeaderMap,
    auth: Auth,
    params: Query<SearchParams>,
) -> Response {
    let tr = Translator::new(locale);
    // Only a request htmx will swap into a real target may get the bare results
    // list: a boosted navigation and a back/forward history replay both target
    // `<body>`, and would make the fragment the entire document.
    let is_htmx = is_fragment_request(&headers);
    let input = params.to_input();
    let query_string = params.query_string();

    let results = match state.search.execute(input).await {
        Ok((page, hit)) => {
            let label = hit
                .as_ref()
                .map(|h| h.label.clone())
                .or_else(|| (!params.q.trim().is_empty()).then(|| params.q.clone()));
            view::build_results(
                tr,
                &page,
                hit.as_ref(),
                label,
                query_string,
                chrono::Utc::now(),
                &state.freshness.thresholds,
                &*state.storage,
            )
            .await
        }
        Err(bikenest_application::SearchError::MissingDestination) => ResultsData {
            destination_label: None,
            total_label: String::new(),
            items: Vec::new(),
            cursor_url: None,
            error: Some(tr.t("search.missing").to_string()),
            map_json: serde_json::json!({ "origin": null, "items": [] }).to_string(),
        },
        // Geocoder outage / rate-limit / bad token → graceful "can't reach the
        // geocoder" page, not a 500 (a hosted provider is a soft dependency).
        Err(bikenest_application::SearchError::Geocode(_)) => ResultsData {
            destination_label: None,
            total_label: String::new(),
            items: Vec::new(),
            cursor_url: None,
            error: Some(tr.t("search.geocode_unavailable").to_string()),
            map_json: serde_json::json!({ "origin": null, "items": [] }).to_string(),
        },
        Err(_) => return internal_error(&headers, &state.map, tr),
    };

    if is_htmx {
        let vm = SearchResultsVm {
            tr,
            results,
            oob: true,
        };
        vary_fragment(render(vm, StatusCode::OK))
    } else {
        let vm = SearchPageVm {
            layout: PageLayout::new(&state.map, tr.t("search.title").to_string(), "search")
                .canonical(format!("{}/search", state.base_url.trim_end_matches('/')))
                .description(tr.t("search.title").to_string())
                .csrf(auth.csrf_value()),
            tr,
            results,
            form: params.0.clone(),
            security_options: view::security_options(tr, Some(&params.security)),
            type_options: view::type_options(tr, Some(&params.parking_type)),
            oob: false,
        };
        // The same URL serves the fragment and the document, chosen by the
        // HX-* headers — say so, or a cache hands one to the wrong request.
        vary_fragment(render(vm, StatusCode::OK))
    }
}

/// Post-action confirmation flags on the details page (`?proposed=1`, `?edited=1`, …).
/// The last four are the no-JS landing spots for the fragment endpoints: with
/// scripting off those POSTs redirect here instead of answering with a partial.
#[derive(Debug, Default, serde::Deserialize)]
struct DetailsNotice {
    #[serde(default)]
    added: Option<String>,
    #[serde(default)]
    edited: Option<String>,
    #[serde(default)]
    proposed: Option<String>,
    #[serde(default)]
    reviewed: Option<String>,
    #[serde(default)]
    verified: Option<String>,
    #[serde(default)]
    parked: Option<String>,
    #[serde(default)]
    reported: Option<String>,
    #[serde(default)]
    photo: Option<String>,
}

/// One notice for the details page banner, newest/strongest action first.
fn details_notice(tr: Translator, q: &DetailsNotice) -> Option<String> {
    if q.proposed.is_some() {
        Some(tr.t("details.notice.proposed").to_string())
    } else if q.edited.is_some() {
        Some(tr.t("details.notice.edited").to_string())
    } else if q.reviewed.is_some() {
        Some(tr.t("details.notice.reviewed").to_string())
    } else if q.added.is_some() {
        Some(tr.t("details.notice.added").to_string())
    } else if q.verified.is_some() {
        Some(tr.t("verification.saved").to_string())
    } else if q.parked.is_some() {
        Some(tr.t("parked.saved").to_string())
    } else if q.reported.is_some() {
        Some(tr.t("report.submitted").to_string())
    } else if q.photo.is_some() {
        Some(tr.t("photo.upload.success").to_string())
    } else {
        None
    }
}

/// P3 — parking details.
async fn parking_details(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(q): Query<DetailsNotice>,
) -> Response {
    let tr = Translator::new(locale);
    match state.details.execute(id).await {
        Ok(Some(view)) => {
            // §25/§46: public P3 returns 404 for a non-ACTIVE location (removed/
            // invalid/flagged). Moderators/admins still see the page with a banner.
            let is_moderator = auth
                .user
                .as_ref()
                .map(|u| u.has_role(Role::Moderator) || u.has_role(Role::Admin))
                .unwrap_or(false);
            if view.location.moderation_state() != ModerationState::Active && !is_moderator {
                return not_found_page(&headers, &state.map, tr);
            }
            // Approved photos (P3 gallery). A read failure degrades to no
            // gallery rather than failing the page.
            let gallery = match state.photos.photos(id).await {
                Ok(photos) => {
                    let name = view.location.name().to_string();
                    let mut gallery = Vec::new();
                    for p in photos {
                        let Some(url) = view::resolve_photo(&*state.storage, Some(&p.key)).await
                        else {
                            continue;
                        };
                        let thumb_url = match p.thumbnail_key.as_deref() {
                            Some(k) => view::resolve_photo(&*state.storage, Some(k))
                                .await
                                .unwrap_or_else(|| url.clone()),
                            None => url.clone(),
                        };
                        gallery.push(PhotoVm {
                            url,
                            thumb_url,
                            alt: p.alt.unwrap_or_else(|| format!("Photo of {name}")),
                        });
                    }
                    gallery
                }
                Err(_) => Vec::new(),
            };
            let viewer = auth.user.as_ref().map(|u| u.id);
            // Community overlay (reviews, confidence, favorite, verification).
            // A read failure degrades to the base detail page, never a 500.
            let community = state
                .contributions
                .community_details(id, viewer)
                .await
                .ok()
                .flatten();
            let verified = auth.user.as_ref().map(|u| u.is_verified).unwrap_or(false);
            // Post-action confirmation (e.g. "this change will be reviewed").
            let notice = details_notice(tr, &q);
            let page = DetailsPage::build_community(
                &state.map,
                tr,
                view,
                gallery,
                auth.csrf_value(),
                community,
                verified,
                auth.authenticated(),
                is_moderator,
                &*state.storage,
            )
            .await
            .notice(notice);
            render(page, StatusCode::OK)
        }
        Ok(None) => not_found_page(&headers, &state.map, tr),
        Err(_) => internal_error(&headers, &state.map, tr),
    }
}

/// Serves an object-storage object behind a signed, expiring URL (Ledger #7,
/// local-disk mode). Invalid/expired signatures and missing objects → 404
/// (never reveal whether a key exists without a valid signature).
#[derive(Debug, serde::Deserialize)]
struct MediaParams {
    #[serde(default)]
    exp: u64,
    #[serde(default)]
    sig: String,
}

async fn media(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Query(params): Query<MediaParams>,
) -> Response {
    if !state.storage.verify_get(&key, params.exp, &params.sig) {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    }
    match state.storage.get(&key).await {
        Ok((bytes, content_type)) => (
            [
                (header::CONTENT_TYPE, content_type),
                (header::CACHE_CONTROL, "private, max-age=3600".to_string()),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

// ---------------------------------------------------------------------------
// M4 photos (upload → validate → process → moderate → publish)
// ---------------------------------------------------------------------------

/// One htmx fragment endpoint's answer, for *any* caller.
///
/// htmx 4 puts `HX-Request` on boosted navigations and history replays too, and
/// both swap `<body>` — so only a real fragment request may be answered with a
/// partial. Everything else (no-JS submit, boosted form post, history replay)
/// gets a whole document: a 303 to the page that now shows the new state on
/// success (post/redirect/get), and the styled error page — at the same status —
/// on failure. htmx 4 swaps 4xx/5xx bodies as well, so the failure fragment is a
/// partial rather than a bare string.
fn fragment_answer(
    headers: &HeaderMap,
    map: &MapConfig,
    tr: Translator,
    status: StatusCode,
    message: &str,
    redirect_to: &str,
    fragment: impl FnOnce() -> Response,
) -> Response {
    if !is_fragment_request(headers) && !status.is_success() {
        return crate::error_response(headers, map, tr, status, message.to_string());
    }
    fragment_or_redirect(headers, fragment(), redirect_to)
}

/// POST /parking/{id}/photo — a verified user uploads one photo (multipart:
/// `photo` file + optional `alt`). Runs the same pipeline as the D1 attach and
/// holds the upload in `PENDING_REVIEW` (§30/§80). Returns a swap-safe fragment.
async fn upload_photo(
    State(state): State<AppState>,
    locale: Locale,
    ClientIp(ip): ClientIp,
    auth: Auth,
    Path(id): Path<i64>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_verified() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let back = format!("/parking/{id}?photo=pending");
    let bad_request = |message: &str| {
        photo_upload_result(
            &headers,
            &state.map,
            tr,
            "error",
            message,
            StatusCode::BAD_REQUEST,
            &back,
        )
    };

    let mut photo_bytes: Option<Vec<u8>> = None;
    let mut alt: Option<String> = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(_) => return bad_request(tr.t("photo.error.internal")),
        };
        match field.name().unwrap_or("") {
            "photo" => match field.bytes().await {
                Ok(b) => photo_bytes = Some(b.to_vec()),
                Err(_) => return bad_request(tr.t("photo.error.internal")),
            },
            "alt" => {
                if let Ok(text) = field.text().await {
                    alt = Some(text);
                }
            }
            _ => {
                // Drain/ignore unknown fields so the connection stays clean.
                let _ = field.bytes().await;
            }
        }
    }

    let Some(bytes) = photo_bytes else {
        return bad_request(tr.t("photo.error.internal"));
    };

    let target = PhotoTarget::Parking(id);
    let (state_name, message, status) = match state
        .photo
        .upload_photo(user, &ip, target, &bytes, alt.as_deref())
        .await
    {
        Ok(_) => (
            "success",
            tr.t("photo.upload.success").to_string(),
            StatusCode::OK,
        ),
        Err(e) => {
            let (status, message) = photo_error(tr, &e);
            ("error", message, status)
        }
    };
    photo_upload_result(
        &headers,
        &state.map,
        tr,
        state_name,
        &message,
        status,
        &back,
    )
}

/// The `?done=…` flag a moderation action redirects a whole-document request
/// back to its queue with. The value names the action, not the message key, so
/// the URL never carries catalog internals.
#[derive(Debug, Default, serde::Deserialize)]
struct ModerationNotice {
    #[serde(default)]
    done: String,
}

/// The queue banner for a `?done=…` flag, or nothing for an unknown value.
fn moderation_notice(tr: Translator, q: &ModerationNotice) -> Option<String> {
    let key = match q.done.as_str() {
        "approved" => "moderation.approved",
        "rejected" => "moderation.rejected",
        "photo_hidden" => "moderation.photo_hidden",
        "photo_restored" => "moderation.photo_restored",
        "claimed" => "report.claimed",
        "resolved" => "report.resolved_msg",
        "dismissed" => "report.dismissed_msg",
        "proposal_approved" => "proposal.approved",
        "proposal_rejected" => "proposal.rejected",
        "review_hidden" => "review.hidden",
        "review_restored" => "review.restored",
        "parking_invalidated" => "parking.invalidated",
        "parking_restored" => "parking.restored",
        _ => return None,
    };
    Some(tr.t(key).to_string())
}

/// GET /moderation/photos — the M2 photo moderation queue (MODERATOR/ADMIN).
async fn moderation_photos(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    Query(q): Query<ModerationNotice>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let items = match state.photo.list_pending_photos(user).await {
        Ok(photos) => {
            let mut items = Vec::with_capacity(photos.len());
            for p in &photos {
                items.push(view::moderation_photo_vm(tr, &*state.storage, p).await);
            }
            items
        }
        Err(_) => Vec::new(),
    };
    render(
        ModerationPhotosPage {
            layout: PageLayout::with_csrf(
                &state.map,
                tr.t("moderation.title").to_string(),
                "moderation",
                auth.csrf_value(),
            ),
            tr,
            items,
            notice: moderation_notice(tr, &q),
        },
        StatusCode::OK,
    )
}

/// Parse a `{kind}` path segment into a [`PhotoKind`].
fn parse_photo_kind(s: &str) -> Option<PhotoKind> {
    PhotoKind::from_code(s)
}

/// The photo queue a moderation action returns a whole-document request to.
fn photo_queue_url(done: &str) -> String {
    format!("/moderation/photos?done={done}")
}

/// POST /moderation/photos/{kind}/{id}/approve (HTMX).
async fn moderation_photo_approve(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, i64)>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let back = photo_queue_url("approved");
    let Some(kind) = parse_photo_kind(&kind) else {
        return photo_upload_result(
            &headers,
            &state.map,
            tr,
            "error",
            tr.t("moderation.invalid"),
            StatusCode::BAD_REQUEST,
            &back,
        );
    };
    let (name, message, status) = match state.photo.approve_photo(user, kind, id).await {
        Ok(()) => (
            "success",
            tr.t("moderation.approved").to_string(),
            StatusCode::OK,
        ),
        Err(e) => {
            let (status, message) = photo_error(tr, &e);
            ("error", message, status)
        }
    };
    photo_upload_result(&headers, &state.map, tr, name, &message, status, &back)
}

/// POST /moderation/photos/{kind}/{id}/reject (HTMX).
async fn moderation_photo_reject(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, i64)>,
    Form(form): Form<RejectReasonForm>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let back = photo_queue_url("rejected");
    let Some(kind) = parse_photo_kind(&kind) else {
        return photo_upload_result(
            &headers,
            &state.map,
            tr,
            "error",
            tr.t("moderation.invalid"),
            StatusCode::BAD_REQUEST,
            &back,
        );
    };
    let (name, message, status) = match state.photo.reject_photo(user, kind, id, &form.reason).await
    {
        Ok(()) => (
            "success",
            tr.t("moderation.rejected").to_string(),
            StatusCode::OK,
        ),
        Err(e) => {
            let (status, message) = photo_error(tr, &e);
            ("error", message, status)
        }
    };
    photo_upload_result(&headers, &state.map, tr, name, &message, status, &back)
}

/// POST /moderation/photos/{kind}/{id}/hide (HTMX) — flips an approved photo to `HIDDEN` (§44).
async fn moderation_photo_hide(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, i64)>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let back = photo_queue_url("photo_hidden");
    let Some(kind) = parse_photo_kind(&kind) else {
        return moderation_result(
            &headers,
            &state.map,
            tr,
            "error",
            tr.t("moderation.invalid"),
            StatusCode::BAD_REQUEST,
            &back,
        );
    };
    let (name, message, status) = match state.moderation.hide_photo(user, kind, id).await {
        Ok(()) => (
            "success",
            tr.t("moderation.photo_hidden").to_string(),
            StatusCode::OK,
        ),
        Err(e) => {
            let (status, message) = moderation_error_message(tr, &e);
            ("error", message, status)
        }
    };
    moderation_result(&headers, &state.map, tr, name, &message, status, &back)
}

/// POST /moderation/photos/{kind}/{id}/restore (HTMX) — flips a hidden photo back to `APPROVED` (§44).
async fn moderation_photo_restore(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, i64)>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let back = photo_queue_url("photo_restored");
    let Some(kind) = parse_photo_kind(&kind) else {
        return moderation_result(
            &headers,
            &state.map,
            tr,
            "error",
            tr.t("moderation.invalid"),
            StatusCode::BAD_REQUEST,
            &back,
        );
    };
    let (name, message, status) = match state.moderation.restore_photo(user, kind, id).await {
        Ok(()) => (
            "success",
            tr.t("moderation.photo_restored").to_string(),
            StatusCode::OK,
        ),
        Err(e) => {
            let (status, message) = moderation_error_message(tr, &e);
            ("error", message, status)
        }
    };
    moderation_result(&headers, &state.map, tr, name, &message, status, &back)
}

// ---------------------------------------------------------------------------
// M5 moderation & reporting handlers
// ---------------------------------------------------------------------------

/// Render a swap-safe moderation-action toast (or, for a whole-document
/// request, redirect back to the queue / show the styled error page).
fn moderation_result(
    headers: &HeaderMap,
    map: &MapConfig,
    tr: Translator,
    state: &'static str,
    message: &str,
    status: StatusCode,
    redirect_to: &str,
) -> Response {
    fragment_answer(headers, map, tr, status, message, redirect_to, || {
        render(
            ModerationActionResultVm {
                tr,
                state,
                message: message.to_string(),
            },
            status,
        )
    })
}

/// Map a [`ModerationError`] to a non-leaking status + friendly message.
fn moderation_error_message(tr: Translator, e: &ModerationError) -> (StatusCode, String) {
    use ModerationError::*;
    let (status, key) = match e {
        NotAuthorized => (StatusCode::FORBIDDEN, "moderation.unauthorized"),
        SelfResolve => (StatusCode::CONFLICT, "moderation.self_resolve"),
        NotFound => (StatusCode::NOT_FOUND, "moderation.not_found"),
        TargetNotFound => (StatusCode::NOT_FOUND, "moderation.target_not_found"),
        InvalidState => (StatusCode::CONFLICT, "moderation.invalid_state"),
        AlreadyReported => (StatusCode::CONFLICT, "report.error.duplicate"),
        StaleProposal => (StatusCode::CONFLICT, "moderation.error.stale_proposal"),
        InvalidReason => (StatusCode::BAD_REQUEST, "report.error.invalid_reason"),
        InvalidField(_) => (StatusCode::BAD_REQUEST, "moderation.invalid"),
        RateLimited => (StatusCode::TOO_MANY_REQUESTS, "report.error.rate_limited"),
        Conflict => (StatusCode::CONFLICT, "error.conflict"),
        Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "error.unavailable"),
        Internal => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "moderation.error.internal",
        ),
    };
    (status, tr.t(key).to_string())
}

fn report_result(
    headers: &HeaderMap,
    map: &MapConfig,
    tr: Translator,
    state: &'static str,
    message: &str,
    status: StatusCode,
    redirect_to: &str,
) -> Response {
    fragment_answer(headers, map, tr, status, message, redirect_to, || {
        render(
            ReportResultVm {
                tr,
                state,
                message: message.to_string(),
            },
            status,
        )
    })
}

/// POST /reports — a user reports content (authenticated; not necessarily verified).
#[derive(Debug, Default, serde::Deserialize)]
struct ReportForm {
    #[serde(default)]
    target_type: String,
    #[serde(default)]
    target_id: i64,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    description: String,
    /// The page the modal was opened from. A report can target a review or a
    /// photo, whose parking id the form does not otherwise carry, so the page
    /// states where a no-JS submit should land. Validated as a local path.
    #[serde(default)]
    page: String,
}

/// Where a whole-document report submit lands: the page the modal was opened
/// from, else the reported spot, else home. Never a caller-controlled origin.
fn report_return_url(form: &ReportForm) -> String {
    if let Some(local) = htmx::safe_local_path(&form.page) {
        let sep = if local.contains('?') { '&' } else { '?' };
        return format!("{local}{sep}reported=1");
    }
    if form.target_type == "parking" && form.target_id > 0 {
        return format!("/parking/{}?reported=1", form.target_id);
    }
    "/".to_string()
}

async fn report_submit(
    State(state): State<AppState>,
    locale: Locale,
    ClientIp(ip): ClientIp,
    auth: Auth,
    headers: HeaderMap,
    Form(form): Form<ReportForm>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_user() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let back = report_return_url(&form);
    if form.target_id <= 0 {
        return report_result(
            &headers,
            &state.map,
            tr,
            "error",
            tr.t("moderation.invalid"),
            StatusCode::BAD_REQUEST,
            &back,
        );
    }
    let Ok(target_type) = ReportTargetType::from_code(&form.target_type) else {
        return report_result(
            &headers,
            &state.map,
            tr,
            "error",
            tr.t("report.error.invalid_reason"),
            StatusCode::BAD_REQUEST,
            &back,
        );
    };
    let description = if form.description.trim().is_empty() {
        None
    } else {
        Some(form.description.clone())
    };
    let (name, message, status) = match state
        .moderation
        .submit_report(
            user,
            &ip,
            target_type,
            form.target_id,
            &form.reason,
            description,
        )
        .await
    {
        Ok(_) => {
            tracing::info!("report submitted"); // §86 (no PII)
            (
                "success",
                tr.t("report.submitted").to_string(),
                StatusCode::OK,
            )
        }
        Err(e) => {
            let (status, message) = moderation_error_message(tr, &e);
            ("error", message, status)
        }
    };
    report_result(&headers, &state.map, tr, name, &message, status, &back)
}

/// GET /moderation — the M1 moderation dashboard (counts + links).
async fn moderation_dashboard(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let pending_photos = state
        .photo
        .list_pending_photos(user)
        .await
        .map(|p| p.len())
        .unwrap_or(0);
    let open_reports = state
        .moderation
        .list_reports(user, Some(ReportState::Open))
        .await
        .map(|r| r.len())
        .unwrap_or(0);
    let under_review_reports = state
        .moderation
        .list_reports(user, Some(ReportState::UnderReview))
        .await
        .map(|r| r.len())
        .unwrap_or(0);
    let pending_proposals = state
        .moderation
        .list_pending_proposals(user)
        .await
        .map(|p| p.len())
        .unwrap_or(0);
    let is_admin = user.has_role(Role::Admin);
    render(
        ModerationDashboardPage {
            layout: PageLayout::with_csrf(
                &state.map,
                tr.t("moderation.dashboard.title").to_string(),
                "moderation",
                auth.csrf_value(),
            ),
            tr,
            pending_photos,
            open_reports,
            under_review_reports,
            pending_proposals,
            is_admin,
        },
        StatusCode::OK,
    )
}

/// GET /moderation/reports — the M3 reports queue (optional `?state=` filter).
#[derive(Debug, Default, serde::Deserialize)]
struct ReportFilterQuery {
    #[serde(default)]
    state: String,
    /// Set by a moderation action redirecting a whole-document request back here.
    #[serde(default)]
    done: String,
}

async fn moderation_reports(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    Query(q): Query<ReportFilterQuery>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let state_filter = if q.state.is_empty() {
        None
    } else {
        ReportState::from_code(&q.state).ok()
    };
    let items = state
        .moderation
        .list_reports(user, state_filter)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|r| view::report_vm(tr, &r))
        .collect();
    render(
        ModerationReportsPage {
            layout: PageLayout::with_csrf(
                &state.map,
                tr.t("moderation.reports.title").to_string(),
                "moderation",
                auth.csrf_value(),
            ),
            tr,
            state_filter: q.state,
            items,
            viewer_id: user.id.0,
            notice: moderation_notice(tr, &ModerationNotice { done: q.done }),
        },
        StatusCode::OK,
    )
}

/// The reports queue a moderation action returns a whole-document request to.
fn reports_queue_url(done: &str) -> String {
    format!("/moderation/reports?done={done}")
}

/// POST /moderation/reports/{id}/claim — claim an open report.
async fn moderation_report_claim(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let (name, message, status) = match state.moderation.claim_report(user, id).await {
        Ok(()) => ("success", tr.t("report.claimed").to_string(), StatusCode::OK),
        Err(e) => {
            let (status, message) = moderation_error_message(tr, &e);
            ("error", message, status)
        }
    };
    moderation_result(
        &headers,
        &state.map,
        tr,
        name,
        &message,
        status,
        &reports_queue_url("claimed"),
    )
}

#[derive(Debug, Default, serde::Deserialize)]
struct ResolutionForm {
    #[serde(default)]
    note: String,
}

/// POST /moderation/reports/{id}/resolve — resolve a claimed report (HTMX).
async fn moderation_report_resolve(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<ResolutionForm>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let (name, message, status) = match state
        .moderation
        .resolve_report(user, id, ReportOutcome::Resolved, &form.note)
        .await
    {
        Ok(()) => {
            tracing::info!("report resolved"); // §86 (no PII)
            (
                "success",
                tr.t("report.resolved_msg").to_string(),
                StatusCode::OK,
            )
        }
        Err(e) => {
            let (status, message) = moderation_error_message(tr, &e);
            ("error", message, status)
        }
    };
    moderation_result(
        &headers,
        &state.map,
        tr,
        name,
        &message,
        status,
        &reports_queue_url("resolved"),
    )
}

/// POST /moderation/reports/{id}/dismiss — dismiss a claimed report (HTMX).
async fn moderation_report_dismiss(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<ResolutionForm>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let (name, message, status) = match state
        .moderation
        .resolve_report(user, id, ReportOutcome::Dismissed, &form.note)
        .await
    {
        Ok(()) => (
            "success",
            tr.t("report.dismissed_msg").to_string(),
            StatusCode::OK,
        ),
        Err(e) => {
            let (status, message) = moderation_error_message(tr, &e);
            ("error", message, status)
        }
    };
    moderation_result(
        &headers,
        &state.map,
        tr,
        name,
        &message,
        status,
        &reports_queue_url("dismissed"),
    )
}

/// GET /moderation/proposals — the M4 proposal review queue.
async fn moderation_proposals(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    Query(q): Query<ModerationNotice>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let items = state
        .moderation
        .list_pending_proposals(user)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|p| view::proposal_vm(tr, &p))
        .collect();
    render(
        ModerationProposalsPage {
            layout: PageLayout::with_csrf(
                &state.map,
                tr.t("moderation.proposals.title").to_string(),
                "moderation",
                auth.csrf_value(),
            ),
            tr,
            items,
            notice: moderation_notice(tr, &q),
        },
        StatusCode::OK,
    )
}

/// The proposals queue a moderation action returns a whole-document request to.
fn proposals_queue_url(done: &str) -> String {
    format!("/moderation/proposals?done={done}")
}

#[derive(Debug, Default, serde::Deserialize)]
struct ApproveProposalForm {
    #[serde(default)]
    lat: String,
    #[serde(default)]
    lon: String,
    #[serde(default)]
    timezone: String,
    #[serde(default)]
    existence: String,
}

/// Build the application to apply. When the approve form carries adjusted values
/// ("modify"), they win; otherwise the proposal's own `proposed` is used.
fn proposal_application(
    proposal: &bikenest_application::Proposal,
    form: &ApproveProposalForm,
) -> Result<ProposalApplication, ModerationError> {
    match proposal.kind {
        ProposalKind::MoveLocation => {
            let lat = if form.lat.trim().is_empty() {
                proposal
                    .proposed
                    .get("lat")
                    .and_then(|v| v.as_f64())
                    .ok_or(ModerationError::InvalidField("lat is required".to_string()))?
            } else {
                form.lat
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| ModerationError::InvalidField("lat is required".to_string()))?
            };
            let lon = if form.lon.trim().is_empty() {
                proposal
                    .proposed
                    .get("lon")
                    .and_then(|v| v.as_f64())
                    .ok_or(ModerationError::InvalidField("lon is required".to_string()))?
            } else {
                form.lon
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| ModerationError::InvalidField("lon is required".to_string()))?
            };
            let tz_raw = if form.timezone.trim().is_empty() {
                proposal
                    .proposed
                    .get("timezone")
                    .and_then(|v| v.as_str())
                    .ok_or(ModerationError::InvalidField(
                        "timezone is required".to_string(),
                    ))?
            } else {
                form.timezone.as_str()
            };
            let timezone = tz_raw
                .parse()
                .map_err(|_| ModerationError::InvalidField("invalid timezone".to_string()))?;
            Ok(ProposalApplication::MoveLocation { lat, lon, timezone })
        }
        ProposalKind::ChangeExistence => {
            let exists = match form.existence.as_str() {
                "removed" => false,
                "exists" => true,
                _ => {
                    let raw = proposal
                        .proposed
                        .get("existence")
                        .and_then(|v| v.as_str())
                        .ok_or(ModerationError::InvalidField(
                            "existence is required".to_string(),
                        ))?;
                    raw != "removed"
                }
            };
            Ok(ProposalApplication::ChangeExistence { exists })
        }
    }
}

/// POST /moderation/proposals/{id}/approve — approve a proposal (optionally adjusted).
async fn moderation_proposal_approve(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<ApproveProposalForm>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let back = proposals_queue_url("proposal_approved");
    let fail = |e: &ModerationError| {
        let (status, message) = moderation_error_message(tr, e);
        moderation_result(&headers, &state.map, tr, "error", &message, status, &back)
    };
    let proposal = match state.moderation.get_proposal(user, id).await {
        Ok(Some(p)) => p,
        _ => return fail(&ModerationError::NotFound),
    };
    let applied = match proposal_application(&proposal, &form) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    match state.moderation.approve_proposal(user, id, applied).await {
        Ok(()) => moderation_result(
            &headers,
            &state.map,
            tr,
            "success",
            tr.t("proposal.approved"),
            StatusCode::OK,
            &back,
        ),
        Err(e) => fail(&e),
    }
}

/// POST /moderation/proposals/{id}/reject — reject with a reason.
async fn moderation_proposal_reject(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<RejectReasonForm>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let (name, message, status) = match state
        .moderation
        .reject_proposal(user, id, &form.reason)
        .await
    {
        Ok(()) => (
            "success",
            tr.t("proposal.rejected").to_string(),
            StatusCode::OK,
        ),
        Err(e) => {
            let (status, message) = moderation_error_message(tr, &e);
            ("error", message, status)
        }
    };
    moderation_result(
        &headers,
        &state.map,
        tr,
        name,
        &message,
        status,
        &proposals_queue_url("proposal_rejected"),
    )
}

/// POST /moderation/reviews/{id}/hide — hide a review.
async fn moderation_review_hide(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let (name, message, status) = match state.moderation.hide_review(user, id).await {
        Ok(()) => ("success", tr.t("review.hidden").to_string(), StatusCode::OK),
        Err(e) => {
            let (status, message) = moderation_error_message(tr, &e);
            ("error", message, status)
        }
    };
    moderation_result(
        &headers,
        &state.map,
        tr,
        name,
        &message,
        status,
        &reports_queue_url("review_hidden"),
    )
}

/// POST /moderation/reviews/{id}/restore — restore a hidden review.
async fn moderation_review_restore(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let (name, message, status) = match state.moderation.restore_review(user, id).await {
        Ok(()) => ("success", tr.t("review.restored").to_string(), StatusCode::OK),
        Err(e) => {
            let (status, message) = moderation_error_message(tr, &e);
            ("error", message, status)
        }
    };
    moderation_result(
        &headers,
        &state.map,
        tr,
        name,
        &message,
        status,
        &reports_queue_url("review_restored"),
    )
}

/// POST /moderation/parking/{id}/invalidate — invalidate a location.
async fn moderation_parking_invalidate(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let (name, message, status) = match state.moderation.invalidate_parking(user, id).await {
        Ok(()) => (
            "success",
            tr.t("parking.invalidated").to_string(),
            StatusCode::OK,
        ),
        Err(e) => {
            let (status, message) = moderation_error_message(tr, &e);
            ("error", message, status)
        }
    };
    moderation_result(
        &headers,
        &state.map,
        tr,
        name,
        &message,
        status,
        &reports_queue_url("parking_invalidated"),
    )
}

/// POST /moderation/parking/{id}/restore — restore an invalid/removed location.
async fn moderation_parking_restore(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let (name, message, status) = match state.moderation.restore_parking(user, id).await {
        Ok(()) => (
            "success",
            tr.t("parking.restored").to_string(),
            StatusCode::OK,
        ),
        Err(e) => {
            let (status, message) = moderation_error_message(tr, &e);
            ("error", message, status)
        }
    };
    moderation_result(
        &headers,
        &state.map,
        tr,
        name,
        &message,
        status,
        &reports_queue_url("parking_restored"),
    )
}

/// POST /admin/users/{id}/suspend — ADMIN-only; revokes sessions + audits.
async fn admin_user_suspend(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<i64>,
) -> Response {
    let actor = match auth.require_role(Role::Admin) {
        Ok(a) => a,
        Err(resp) => return resp,
    };
    match state.auth.suspend_user(actor, UserId(id)).await {
        Ok(()) => axum::response::Redirect::to("/admin/users?suspended=1").into_response(),
        Err(_) => axum::response::Redirect::to("/admin/users?error=1").into_response(),
    }
}

/// POST /admin/users/{id}/restore — ADMIN-only; restores to Active + audits.
async fn admin_user_restore(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<i64>,
) -> Response {
    let actor = match auth.require_role(Role::Admin) {
        Ok(a) => a,
        Err(resp) => return resp,
    };
    match state.auth.restore_user(actor, UserId(id)).await {
        Ok(()) => axum::response::Redirect::to("/admin/users?restored=1").into_response(),
        Err(_) => axum::response::Redirect::to("/admin/users?error=1").into_response(),
    }
}

/// GET /admin/users/{id}/contributions — a target user's C5 feed (MODERATOR/ADMIN).
async fn admin_user_contributions(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    Path(id): Path<i64>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let target = UserId(id);
    let email = state
        .auth
        .list_users()
        .await
        .ok()
        .and_then(|users| users.into_iter().find(|u| u.id == target))
        .map(|u| u.email.to_string())
        .unwrap_or_else(|| format!("#{id}"));
    let items = state
        .moderation
        .user_contribution_history(user, target)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|i| view::contribution_vm(tr, &i))
        .collect();
    render(
        AdminUserContributionsPage {
            layout: PageLayout::with_csrf(
                &state.map,
                tr.t("admin.contrib.title").to_string(),
                "admin",
                auth.csrf_value(),
            ),
            tr,
            user_id: id,
            email,
            items,
        },
        StatusCode::OK,
    )
}

#[derive(Debug, Default, serde::Deserialize)]
struct AuditFilterQuery {
    #[serde(default)]
    action: String,
    #[serde(default)]
    target_type: String,
    #[serde(default)]
    actor: i64,
    #[serde(default)]
    from: String,
    #[serde(default)]
    to: String,
    #[serde(default)]
    cursor: i64,
}

/// GET /admin/audit — the ADMIN-only audit-log viewer.
async fn admin_audit(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    Query(q): Query<AuditFilterQuery>,
) -> Response {
    let tr = Translator::new(locale);
    let admin = match auth.require_role(Role::Admin) {
        Ok(a) => a,
        Err(resp) => return resp,
    };
    let filter = AuditFilter {
        actor_id: (q.actor > 0).then_some(UserId(q.actor)),
        action: (!q.action.is_empty()).then(|| q.action.clone()),
        target_type: (!q.target_type.is_empty()).then(|| q.target_type.clone()),
        from: parse_datetime(&q.from),
        to: parse_datetime(&q.to),
        cursor: (q.cursor > 0).then_some(q.cursor),
        limit: 50,
    };
    let page = state
        .moderation
        .list_audit_events(admin, filter)
        .await
        .map(|p| (p.items, p.next_cursor))
        .unwrap_or_default();
    let items = page
        .0
        .into_iter()
        .map(|e| view::audit_row_vm(tr, &e))
        .collect();
    render(
        AdminAuditPage {
            layout: PageLayout::with_csrf(
                &state.map,
                tr.t("admin.audit.title").to_string(),
                "admin",
                auth.csrf_value(),
            ),
            tr,
            items,
            next_cursor: page.1,
            action: q.action.clone(),
            target_type: q.target_type.clone(),
            actor: q.actor.to_string(),
            from: q.from.clone(),
            to: q.to.clone(),
            notice: None,
        },
        StatusCode::OK,
    )
}

fn parse_datetime(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

/// A tiny rejected-photo form: the moderator's reason.
#[derive(Debug, serde::Deserialize)]
struct RejectReasonForm {
    #[serde(default)]
    reason: String,
}

fn photo_upload_result(
    headers: &HeaderMap,
    map: &MapConfig,
    tr: Translator,
    state: &'static str,
    message: &str,
    status: StatusCode,
    redirect_to: &str,
) -> Response {
    fragment_answer(headers, map, tr, status, message, redirect_to, || {
        render(
            crate::PhotoUploadResultVm {
                tr,
                state,
                message: message.to_string(),
            },
            status,
        )
    })
}

/// Map a [`PhotoError`] to a non-leaking status + friendly message.
fn photo_error(tr: Translator, e: &PhotoError) -> (StatusCode, String) {
    use PhotoError::*;
    let (status, key) = match e {
        NotVerified => (StatusCode::FORBIDDEN, "photo.error.not_verified"),
        RateLimited => (StatusCode::TOO_MANY_REQUESTS, "photo.error.rate_limited"),
        TooLarge => (StatusCode::BAD_REQUEST, "photo.error.too_large"),
        UnsupportedFormat => (StatusCode::BAD_REQUEST, "photo.error.unsupported"),
        Undecodable => (StatusCode::BAD_REQUEST, "photo.error.undecodable"),
        TooManyPixels => (StatusCode::BAD_REQUEST, "photo.error.too_many_pixels"),
        NotFound => (StatusCode::NOT_FOUND, "photo.error.not_found"),
        NotPending => (StatusCode::CONFLICT, "moderation.not_pending"),
        Unauthorized => (StatusCode::FORBIDDEN, "moderation.unauthorized"),
        InvalidField(_) => (StatusCode::BAD_REQUEST, "photo.error.invalid"),
        Storage(_) => (StatusCode::INTERNAL_SERVER_ERROR, "photo.error.internal"),
        Conflict => (StatusCode::CONFLICT, "error.conflict"),
        Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "error.unavailable"),
        Internal => (StatusCode::INTERNAL_SERVER_ERROR, "photo.error.internal"),
    };
    (status, tr.t(key).to_string())
}

/// P7 — about / how it works.
async fn about(State(state): State<AppState>, locale: Locale, auth: Auth) -> Response {
    let tr = Translator::new(locale);
    let page = AboutPage {
        layout: PageLayout::new(&state.map, tr.t("about.title").to_string(), "about")
            .canonical(format!("{}/about", state.base_url.trim_end_matches('/')))
            .description(tr.t("about.title").to_string())
            .csrf(auth.csrf_value()),
        tr,
    };
    render(page, StatusCode::OK)
}

/// Language toggle (§12): set the `lang` cookie and return to `next` (a local
/// path only) or the referring page. Unknown codes just redirect home.
#[derive(Debug, serde::Deserialize)]
struct LangParams {
    #[serde(default)]
    next: String,
}

async fn set_lang(
    Path(code): Path<String>,
    Query(params): Query<LangParams>,
    headers: HeaderMap,
) -> Response {
    // Return the user to where they were: explicit `next`, else the page htmx
    // reports (`HX-Current-URL`), else the Referer — all reduced through the one
    // open-redirect guard, which also rejects the `\` a browser may normalise
    // into a second `/` (`/\evil.com`).
    let from_header = |name: &str| -> Option<String> {
        let raw = headers.get(name)?.to_str().ok()?;
        // Strip scheme + host to keep only the local path (+ query).
        let path = raw.find("://").map(|i| {
            raw[i + 3..]
                .find('/')
                .map(|j| &raw[i + 3 + j..])
                .unwrap_or("/")
        });
        htmx::safe_local_path(path.unwrap_or(raw)).map(str::to_string)
    };
    let next = htmx::safe_local_path(&params.next)
        .map(str::to_string)
        .or_else(|| from_header(htmx::HX_CURRENT_URL))
        .or_else(|| from_header("referer"))
        .unwrap_or_else(|| "/".to_string());
    let Some(locale) = Locale::from_code(&code) else {
        return axum::response::Redirect::to(&next).into_response();
    };
    let cookie = format!(
        "lang={}; Path=/; Max-Age=31536000; SameSite=Lax",
        locale.code()
    );
    (
        [(header::SET_COOKIE, cookie)],
        axum::response::Redirect::to(&next),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Accounts & authentication handlers (M2)
// ---------------------------------------------------------------------------

fn redirect_with_cookie(path: &str, cookie: &str) -> Response {
    (
        [(header::SET_COOKIE, cookie)],
        axum::response::Redirect::to(path),
    )
        .into_response()
}

fn random_state_hex() -> String {
    let bytes = RealTokenGenerator.generate();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn auth_error_message(tr: Translator, err: &AuthError) -> String {
    match err {
        AuthError::WeakPassword => tr.t("auth.error.weak_password").to_string(),
        AuthError::InvalidEmail => tr.t("auth.error.invalid_email").to_string(),
        AuthError::RateLimited => tr.t("auth.error.rate_limited").to_string(),
        AuthError::TokenExpired | AuthError::TokenUsed | AuthError::TokenInvalid => {
            tr.t("auth.error.invalid_token").to_string()
        }
        AuthError::RefuseAdminSelfRevoke => tr.t("auth.error.last_admin").to_string(),
        AuthError::Conflict => tr.t("error.conflict").to_string(),
        AuthError::Unavailable => tr.t("error.unavailable").to_string(),
        _ => tr.t("auth.error.generic").to_string(),
    }
}

fn format_roles(tr: Translator, mut roles: Vec<Role>) -> String {
    roles.sort();
    roles.dedup();
    roles
        .iter()
        .map(|r| view::role_label(tr, *r))
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Debug, Default, serde::Deserialize)]
struct RegisterForm {
    #[serde(default)]
    email: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    password: String,
}

async fn register_page(State(state): State<AppState>, locale: Locale, auth: Auth) -> Response {
    if auth.authenticated() {
        return axum::response::Redirect::to("/account").into_response();
    }
    let tr = Translator::new(locale);
    let token = anon_csrf_token();
    render_anon(
        RegisterPage {
            layout: PageLayout::new(&state.map, tr.t("auth.register_title").to_string(), "auth")
                .csrf(token.clone()),
            tr,
            email: String::new(),
            display_name: String::new(),
            error: None,
        },
        &token,
    )
}

#[allow(clippy::too_many_arguments)]
async fn register_post(
    State(state): State<AppState>,
    locale: Locale,
    ClientIp(ip): ClientIp,
    auth: Auth,
    Form(form): Form<RegisterForm>,
) -> Response {
    if auth.authenticated() {
        return axum::response::Redirect::to("/account").into_response();
    }
    let tr = Translator::new(locale);
    let display_name = if form.display_name.trim().is_empty() {
        None
    } else {
        Some(form.display_name.trim())
    };
    match state
        .auth
        .register(&ip, &form.email, display_name, &form.password)
        .await
    {
        Ok(()) => axum::response::Redirect::to("/login?registered=1").into_response(),
        Err(err) => {
            // Re-render with a fresh double-submit CSRF token so the next POST validates.
            let token = anon_csrf_token();
            render_anon(
                RegisterPage {
                    layout: PageLayout::new(
                        &state.map,
                        tr.t("auth.register_title").to_string(),
                        "auth",
                    )
                    .csrf(token.clone()),
                    tr,
                    email: form.email,
                    display_name: form.display_name,
                    error: Some(auth_error_message(tr, &err)),
                },
                &token,
            )
        }
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct LoginNotices {
    #[serde(default)]
    registered: Option<String>,
    #[serde(default)]
    verified: Option<String>,
    #[serde(default)]
    reset: Option<String>,
    #[serde(default)]
    resend: Option<String>,
    #[serde(default)]
    oauth: Option<String>,
    /// Where the gate that bounced the user wanted them to end up.
    #[serde(default)]
    next: String,
}

/// Build the notice shown on the login page from a query-string flag.
fn login_notice(tr: Translator, q: &LoginNotices) -> Option<String> {
    if q.registered.is_some() {
        Some(tr.t("auth.registered").to_string())
    } else if q.verified.is_some() {
        Some(tr.t("auth.verified").to_string())
    } else if q.reset.is_some() {
        Some(tr.t("auth.reset_sent").to_string())
    } else if q.resend.is_some() {
        Some(tr.t("auth.resend_sent").to_string())
    } else if q.oauth.is_some() {
        Some(tr.t("auth.oauth_failed").to_string())
    } else {
        None
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct LoginForm {
    #[serde(default)]
    email: String,
    #[serde(default)]
    password: String,
    /// Carried across the round trip by the hidden field on the login form.
    #[serde(default)]
    next: String,
}

/// Where a login lands. `next` comes from the user (a query parameter, then a
/// form field), so it is reduced to a safe local path or dropped — `/account`
/// is the default and the only answer for `//evil.com` or `/\evil.com`.
fn login_destination(next: &str) -> String {
    htmx::safe_local_path(next)
        .unwrap_or("/account")
        .to_string()
}

async fn login_page(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    Query(q): Query<LoginNotices>,
) -> Response {
    if auth.authenticated() {
        return axum::response::Redirect::to(&login_destination(&q.next)).into_response();
    }
    let tr = Translator::new(locale);
    let token = anon_csrf_token();
    render_anon(
        LoginPage {
            layout: PageLayout::new(&state.map, tr.t("auth.login_title").to_string(), "auth")
                .csrf(token.clone()),
            tr,
            email: String::new(),
            notice: login_notice(tr, &q),
            error: None,
            next: htmx::safe_local_path(&q.next).unwrap_or("").to_string(),
            google_enabled: state.google_oauth_enabled,
        },
        &token,
    )
}

async fn login_post(
    State(state): State<AppState>,
    locale: Locale,
    ClientIp(ip): ClientIp,
    auth: Auth,
    Query(q): Query<LoginNotices>,
    Form(form): Form<LoginForm>,
) -> Response {
    // The form field wins (it survives a failed attempt); the query parameter
    // is the fallback for a link straight to `/login?next=…`.
    let next = if form.next.is_empty() {
        q.next.clone()
    } else {
        form.next.clone()
    };
    if auth.authenticated() {
        return axum::response::Redirect::to(&login_destination(&next)).into_response();
    }
    let tr = Translator::new(locale);
    match state.auth.login(&ip, &form.email, &form.password).await {
        Ok(outcome) => redirect_with_cookie(
            &login_destination(&next),
            &set_session_cookie(&outcome.session),
        ),
        // One generic message for bad credentials AND suspended/deleted (§45).
        // The submitted email is NOT echoed back, so the failure response is
        // byte-identical whether or not the account exists — and it still
        // carries a fresh double-submit CSRF token for the next attempt.
        Err(_) => {
            tracing::warn!("login failed"); // §86: no email/IP/PII in the log field
            let token = anon_csrf_token();
            render_anon(
                LoginPage {
                    layout: PageLayout::new(
                        &state.map,
                        tr.t("auth.login_title").to_string(),
                        "auth",
                    )
                    .csrf(token.clone()),
                    tr,
                    email: String::new(),
                    notice: None,
                    error: Some(tr.t("auth.error.invalid_credentials").to_string()),
                    next: htmx::safe_local_path(&next).unwrap_or("").to_string(),
                    google_enabled: state.google_oauth_enabled,
                },
                &token,
            )
        }
    }
}

async fn logout(State(state): State<AppState>, auth: Auth) -> Response {
    if let Some(session) = &auth.session {
        let _ = state.auth.logout(session).await;
    }
    (
        [(header::SET_COOKIE, clear_session_cookie())],
        axum::response::Redirect::to("/"),
    )
        .into_response()
}

#[derive(Debug, Default, serde::Deserialize)]
struct VerifyParams {
    #[serde(default)]
    token: Option<String>,
}

async fn verify_email(
    State(state): State<AppState>,
    locale: Locale,
    Query(q): Query<VerifyParams>,
) -> Response {
    let tr = Translator::new(locale);
    let Some(token) = q.token.filter(|t| !t.is_empty()) else {
        let t = anon_csrf_token();
        return render_anon(
            VerifyEmailPage {
                layout: PageLayout::new(&state.map, tr.t("auth.verify_title").to_string(), "auth")
                    .csrf(t.clone()),
                tr,
                success: false,
                error: Some(tr.t("auth.error.invalid_token").to_string()),
            },
            &t,
        );
    };
    match state.auth.verify_email(&token).await {
        Ok(()) => axum::response::Redirect::to("/login?verified=1").into_response(),
        Err(err) => {
            let t = anon_csrf_token();
            render_anon(
                VerifyEmailPage {
                    layout: PageLayout::new(
                        &state.map,
                        tr.t("auth.verify_title").to_string(),
                        "auth",
                    )
                    .csrf(t.clone()),
                    tr,
                    success: false,
                    error: Some(auth_error_message(tr, &err)),
                },
                &t,
            )
        }
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct ResendForm {
    #[serde(default)]
    email: String,
}

async fn verify_resend(
    State(state): State<AppState>,
    locale: Locale,
    ClientIp(ip): ClientIp,
    Form(form): Form<ResendForm>,
) -> Response {
    let tr = Translator::new(locale);
    let Ok(email) = UserEmail::parse(&form.email) else {
        return axum::response::Redirect::to("/login?resend=1").into_response();
    };
    match state.auth.resend_verification(&ip, &email).await {
        Ok(()) => axum::response::Redirect::to("/login?resend=1").into_response(),
        Err(err) => {
            let t = anon_csrf_token();
            render_anon(
                LoginPage {
                    layout: PageLayout::new(
                        &state.map,
                        tr.t("auth.login_title").to_string(),
                        "auth",
                    )
                    .csrf(t.clone()),
                    tr,
                    email: String::new(),
                    notice: None,
                    error: Some(auth_error_message(tr, &err)),
                    next: String::new(),
                    google_enabled: state.google_oauth_enabled,
                },
                &t,
            )
        }
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct ResetRequestForm {
    #[serde(default)]
    email: String,
}

#[derive(Debug, Default, serde::Deserialize)]
struct ResetSent {
    #[serde(default)]
    sent: Option<String>,
}

async fn password_reset_page(
    State(state): State<AppState>,
    locale: Locale,
    Query(q): Query<ResetSent>,
) -> Response {
    let tr = Translator::new(locale);
    let notice = if q.sent.is_some() {
        Some(tr.t("auth.reset_sent").to_string())
    } else {
        None
    };
    let token = anon_csrf_token();
    render_anon(
        PasswordResetPage {
            layout: PageLayout::new(&state.map, tr.t("auth.reset_title").to_string(), "auth")
                .csrf(token.clone()),
            tr,
            email: String::new(),
            notice,
            error: None,
        },
        &token,
    )
}

async fn password_reset_post(
    State(state): State<AppState>,
    locale: Locale,
    ClientIp(ip): ClientIp,
    Form(form): Form<ResetRequestForm>,
) -> Response {
    let tr = Translator::new(locale);
    let Ok(email) = UserEmail::parse(&form.email) else {
        return axum::response::Redirect::to("/password-reset?sent=1").into_response();
    };
    match state.auth.request_password_reset(&ip, &email).await {
        Ok(()) => axum::response::Redirect::to("/password-reset?sent=1").into_response(),
        Err(err) => {
            let t = anon_csrf_token();
            render_anon(
                PasswordResetPage {
                    layout: PageLayout::new(
                        &state.map,
                        tr.t("auth.reset_title").to_string(),
                        "auth",
                    )
                    .csrf(t.clone()),
                    tr,
                    email: form.email,
                    notice: None,
                    error: Some(auth_error_message(tr, &err)),
                },
                &t,
            )
        }
    }
}

async fn password_reset_new(
    State(state): State<AppState>,
    locale: Locale,
    Query(q): Query<VerifyParams>,
) -> Response {
    let tr = Translator::new(locale);
    let token = q.token.unwrap_or_default();
    let t = anon_csrf_token();
    render_anon(
        PasswordResetNewPage {
            layout: PageLayout::new(&state.map, tr.t("auth.reset_new_title").to_string(), "auth")
                .csrf(t.clone()),
            tr,
            token,
            error: None,
        },
        &t,
    )
}

#[derive(Debug, Default, serde::Deserialize)]
struct ResetNewForm {
    #[serde(default)]
    token: String,
    #[serde(default)]
    password: String,
}

async fn password_reset_new_post(
    State(state): State<AppState>,
    locale: Locale,
    Form(form): Form<ResetNewForm>,
) -> Response {
    let tr = Translator::new(locale);
    match state.auth.reset_password(&form.token, &form.password).await {
        Ok(()) => axum::response::Redirect::to("/login?reset=1").into_response(),
        Err(err) => {
            let t = anon_csrf_token();
            render_anon(
                PasswordResetNewPage {
                    layout: PageLayout::new(
                        &state.map,
                        tr.t("auth.reset_new_title").to_string(),
                        "auth",
                    )
                    .csrf(t.clone()),
                    tr,
                    token: form.token,
                    error: Some(auth_error_message(tr, &err)),
                },
                &t,
            )
        }
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct ConsentParams {
    #[serde(default)]
    state: String,
}

async fn auth_google(State(state): State<AppState>) -> Response {
    let state_val = random_state_hex();
    let url = state.auth.oauth_authorize_url(&state_val);
    axum::response::Redirect::to(&url).into_response()
}

/// The fake provider's "consent" page (Ledger #5): auto-issues a code that
/// redirects to the real callback route.
async fn auth_google_fake_consent(Query(q): Query<ConsentParams>) -> Response {
    axum::response::Redirect::to(&format!(
        "/auth/google/callback?code=fake-oauth-code&state={}",
        q.state
    ))
    .into_response()
}

#[derive(Debug, Default, serde::Deserialize)]
struct CallbackParams {
    #[serde(default)]
    code: String,
    #[serde(default)]
    state: String,
}

async fn auth_google_callback(
    State(state): State<AppState>,
    Query(q): Query<CallbackParams>,
) -> Response {
    if q.state.is_empty() {
        return axum::response::Redirect::to("/login?oauth=error").into_response();
    }
    match state.auth.oauth_callback(&q.code).await {
        Ok(outcome) => redirect_with_cookie("/account", &set_session_cookie(&outcome.session)),
        Err(_) => axum::response::Redirect::to("/login?oauth=error").into_response(),
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct AccountNotices {
    #[serde(default)]
    pw_changed: Option<String>,
    #[serde(default)]
    email_pending: Option<String>,
}

async fn account(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    Query(q): Query<AccountNotices>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_user() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let notice = if q.pw_changed.is_some() {
        Some(tr.t("account.pw_changed").to_string())
    } else if q.email_pending.is_some() {
        Some(tr.t("account.email_pending").to_string())
    } else {
        None
    };
    render(
        AccountPage {
            layout: PageLayout::with_csrf(
                &state.map,
                tr.t("account.title").to_string(),
                "account",
                auth.csrf_value(),
            ),
            tr,
            email: user.email.to_string(),
            display_name: user.display_name.clone(),
            is_verified: user.is_verified,
            roles_label: format_roles(tr, user.roles.clone()),
            notice,
        },
        StatusCode::OK,
    )
}

#[derive(Debug, Default, serde::Deserialize)]
struct ChangePasswordForm {
    #[serde(default)]
    current_password: String,
    #[serde(default)]
    new_password: String,
}

async fn account_password(State(state): State<AppState>, locale: Locale, auth: Auth) -> Response {
    let tr = Translator::new(locale);
    if let Err(resp) = auth.require_user() {
        return resp;
    }
    render(
        AccountPasswordPage {
            layout: PageLayout::with_csrf(
                &state.map,
                tr.t("account.pw_title").to_string(),
                "account",
                auth.csrf_value(),
            ),
            tr,
            error: None,
            notice: None,
        },
        StatusCode::OK,
    )
}

async fn account_password_post(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    Form(form): Form<ChangePasswordForm>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_user() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let session = auth.session.as_ref();
    let session = match session {
        Some(s) => s,
        None => return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };
    match state
        .auth
        .change_password(user.id, &form.current_password, &form.new_password, session)
        .await
    {
        Ok(()) => axum::response::Redirect::to("/account?pw_changed=1").into_response(),
        Err(err) => render(
            AccountPasswordPage {
                layout: PageLayout::with_csrf(
                    &state.map,
                    tr.t("account.pw_title").to_string(),
                    "account",
                    auth.csrf_value(),
                ),
                tr,
                error: Some(auth_error_message(tr, &err)),
                notice: None,
            },
            StatusCode::OK,
        ),
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct ChangeEmailForm {
    #[serde(default)]
    current_password: String,
    #[serde(default)]
    new_email: String,
}

async fn account_email(State(state): State<AppState>, locale: Locale, auth: Auth) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_user() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    render(
        AccountEmailPage {
            layout: PageLayout::with_csrf(
                &state.map,
                tr.t("account.email_title").to_string(),
                "account",
                auth.csrf_value(),
            ),
            tr,
            email: user.email.to_string(),
            error: None,
            notice: None,
        },
        StatusCode::OK,
    )
}

async fn account_email_post(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    Form(form): Form<ChangeEmailForm>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_user() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let Ok(new_email) = UserEmail::parse(&form.new_email) else {
        return render(
            AccountEmailPage {
                layout: PageLayout::with_csrf(
                    &state.map,
                    tr.t("account.email_title").to_string(),
                    "account",
                    auth.csrf_value(),
                ),
                tr,
                email: user.email.to_string(),
                error: Some(tr.t("auth.error.invalid_email").to_string()),
                notice: None,
            },
            StatusCode::OK,
        );
    };
    match state
        .auth
        .change_email(user.id, &form.current_password, &new_email)
        .await
    {
        Ok(()) => axum::response::Redirect::to("/account?email_pending=1").into_response(),
        Err(err) => render(
            AccountEmailPage {
                layout: PageLayout::with_csrf(
                    &state.map,
                    tr.t("account.email_title").to_string(),
                    "account",
                    auth.csrf_value(),
                ),
                tr,
                email: user.email.to_string(),
                error: Some(auth_error_message(tr, &err)),
                notice: None,
            },
            StatusCode::OK,
        ),
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct AdminNotices {
    #[serde(default)]
    granted: Option<String>,
    #[serde(default)]
    revoked: Option<String>,
    #[serde(default)]
    suspended: Option<String>,
    #[serde(default)]
    restored: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

async fn admin_users(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    Query(q): Query<AdminNotices>,
) -> Response {
    let tr = Translator::new(locale);
    match auth.require_role(Role::Admin) {
        Ok(_) => {}
        Err(resp) => return resp,
    }
    let users = match state.auth.list_users().await {
        Ok(users) => view::admin_users(tr, &users),
        Err(_) => Vec::new(),
    };
    render(
        AdminUsersPage {
            layout: PageLayout::with_csrf(
                &state.map,
                tr.t("admin.users_title").to_string(),
                "admin",
                auth.csrf_value(),
            ),
            tr,
            users,
            notice: if q.granted.is_some() {
                Some(tr.t("admin.granted").to_string())
            } else if q.revoked.is_some() {
                Some(tr.t("admin.revoked").to_string())
            } else if q.suspended.is_some() {
                Some(tr.t("admin.suspended").to_string())
            } else if q.restored.is_some() {
                Some(tr.t("admin.restored").to_string())
            } else {
                None
            },
            error: q
                .error
                .as_ref()
                .map(|_| tr.t("admin.role_error").to_string()),
        },
        StatusCode::OK,
    )
}

#[derive(Debug, Default, serde::Deserialize)]
struct RoleForm {
    #[serde(default)]
    action: String,
    #[serde(default)]
    role: String,
}

async fn admin_role_post(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<i64>,
    Form(form): Form<RoleForm>,
) -> Response {
    let actor = match auth.require_role(Role::Admin) {
        Ok(a) => a,
        Err(resp) => return resp,
    };
    let target = UserId(id);
    let Some(role) = Role::from_code(&form.role) else {
        return axum::response::Redirect::to("/admin/users?error=1").into_response();
    };
    let result = match form.action.as_str() {
        "grant" => state.auth.grant_role(actor, target, role).await,
        "revoke" => state.auth.revoke_role(actor, target, role).await,
        _ => return axum::response::Redirect::to("/admin/users?error=1").into_response(),
    };
    match result {
        Ok(()) => {
            let path = if form.action == "grant" {
                "/admin/users?granted=1"
            } else {
                "/admin/users?revoked=1"
            };
            axum::response::Redirect::to(path).into_response()
        }
        Err(_) => axum::response::Redirect::to("/admin/users?error=1").into_response(),
    }
}

// ---------------------------------------------------------------------------
// M6 — Privacy & account lifecycle
// ---------------------------------------------------------------------------

/// Resolve the current document for the request locale, falling back to
/// pt-BR (§102) when that locale has no published document.
async fn current_policy(
    state: &AppState,
    kind: PolicyKind,
    locale: Locale,
) -> Option<bikenest_application::PolicyDocument> {
    let code = locale.html_lang();
    match state.policy.current(kind, code).await {
        Ok(Some(doc)) => Some(doc),
        Ok(None) if code != POLICY_FALLBACK_LOCALE => state
            .policy
            .current(kind, POLICY_FALLBACK_LOCALE)
            .await
            .ok()
            .flatten(),
        _ => None,
    }
}

/// Version history for the request locale, falling back to pt-BR (§102).
async fn policy_history(
    state: &AppState,
    kind: PolicyKind,
    locale: Locale,
) -> Vec<bikenest_application::PolicyDocument> {
    let code = locale.html_lang();
    let docs = state.policy.history(kind, code).await.unwrap_or_default();
    if docs.is_empty() && code != POLICY_FALLBACK_LOCALE {
        return state
            .policy
            .history(kind, POLICY_FALLBACK_LOCALE)
            .await
            .unwrap_or_default();
    }
    docs
}

fn policy_kind_meta(tr: Translator, kind: PolicyKind) -> (&'static str, &'static str) {
    match kind {
        PolicyKind::Privacy => ("privacy", tr.t("nav.privacy")),
        PolicyKind::Terms => ("terms", tr.t("nav.terms")),
        PolicyKind::Cookies => ("cookies", tr.t("nav.cookies")),
    }
}

/// Shared builder for a public versioned legal page (P4/P5/P6). The stored
/// markdown goes through [`crate::markdown::render_policy_markdown`], which
/// escapes raw HTML — that output is the only `|safe` value in the template.
async fn policy_page_impl(state: &AppState, locale: Locale, kind: PolicyKind) -> Response {
    let tr = Translator::new(locale);
    let (kind_code, kind_label) = policy_kind_meta(tr, kind);
    let layout = PageLayout::new(&state.map, format!("{kind_label} — BikeNest"), kind_code);
    match current_policy(state, kind, locale).await {
        Some(doc) => render(
            PolicyPage {
                layout,
                tr,
                kind_code,
                kind_label,
                version: doc.version.clone(),
                effective_label: view::iso_datetime_label(tr, doc.effective_at),
                content: crate::markdown::render_policy_markdown(&doc.content),
            },
            StatusCode::OK,
        ),
        None => render(
            PolicyPage {
                layout,
                tr,
                kind_code,
                kind_label,
                version: "—".to_string(),
                effective_label: String::new(),
                content: format!(
                    "<p>{}</p>",
                    crate::markdown::escape_text(tr.t("policy.missing"))
                ),
            },
            StatusCode::OK,
        ),
    }
}

async fn privacy_page(locale: Locale, State(state): State<AppState>) -> Response {
    policy_page_impl(&state, locale, PolicyKind::Privacy).await
}
async fn terms_page(locale: Locale, State(state): State<AppState>) -> Response {
    policy_page_impl(&state, locale, PolicyKind::Terms).await
}
async fn cookies_page(locale: Locale, State(state): State<AppState>) -> Response {
    policy_page_impl(&state, locale, PolicyKind::Cookies).await
}

/// Shared builder for a policy version-history page (§70).
async fn policy_versions_impl(state: &AppState, locale: Locale, kind: PolicyKind) -> Response {
    let tr = Translator::new(locale);
    let (kind_code, kind_label) = policy_kind_meta(tr, kind);
    let docs = policy_history(state, kind, locale).await;
    let items: Vec<view::PolicyVersionVm> = docs
        .iter()
        .map(|d| view::policy_version_vm(tr, d))
        .collect();
    render(
        PolicyVersionsPage {
            layout: PageLayout::new(
                &state.map,
                format!("{} — BikeNest", tr.t("policy.versions_title")),
                kind_code,
            ),
            tr,
            kind_code,
            kind_label,
            items,
        },
        StatusCode::OK,
    )
}

async fn privacy_versions(locale: Locale, State(state): State<AppState>) -> Response {
    policy_versions_impl(&state, locale, PolicyKind::Privacy).await
}
async fn terms_versions(locale: Locale, State(state): State<AppState>) -> Response {
    policy_versions_impl(&state, locale, PolicyKind::Terms).await
}
async fn cookies_versions(locale: Locale, State(state): State<AppState>) -> Response {
    policy_versions_impl(&state, locale, PolicyKind::Cookies).await
}

/// C6 — privacy & data hub.
async fn account_privacy(State(state): State<AppState>, locale: Locale, auth: Auth) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_user() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let _ = user;
    let notice = None;
    render(
        AccountPrivacyPage {
            layout: PageLayout::with_csrf(
                &state.map,
                tr.t("privacy.hub_title").to_string(),
                "account",
                auth.csrf_value(),
            ),
            tr,
            request_types: view::privacy_request_kind_options(tr),
            consent_records: false,
            notice,
        },
        StatusCode::OK,
    )
}

/// POST /account/privacy/export — request a personal-data export.
async fn account_export_post(
    State(state): State<AppState>,
    _locale: Locale,
    auth: Auth,
) -> Response {
    let user = match auth.require_user() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    match state.privacy.request_export(user).await {
        Ok(req) => {
            Redirect::to(&format!("/account/export/{}?token={}", req.id, req.token)).into_response()
        }
        Err(_) => Redirect::to("/account/privacy?export_error=1").into_response(),
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct RightsRequestForm {
    kind: String,
    #[serde(default)]
    details: String,
}

/// POST /account/privacy/request — submit a manual rights request (§72).
async fn account_privacy_request_post(
    State(state): State<AppState>,
    _locale: Locale,
    auth: Auth,
    Form(form): Form<RightsRequestForm>,
) -> Response {
    let user = match auth.require_user() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let kind = match PrivacyRequestKind::from_code(&form.kind) {
        Ok(k) => k,
        Err(_) => return Redirect::to("/account/privacy?request_error=1").into_response(),
    };
    let details = if form.details.trim().is_empty() {
        json!({})
    } else {
        json!({ "note": form.details.trim() })
    };
    match state.privacy.submit_request(user, kind, details).await {
        Ok(_) => {
            tracing::info!("privacy request submitted"); // §86 (no PII)
            Redirect::to("/account/privacy?requested=1").into_response()
        }
        Err(_) => Redirect::to("/account/privacy?request_error=1").into_response(),
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct ExportQuery {
    token: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// C7 — export status + single-use download link.
async fn account_export(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    Path(id): Path<i64>,
    Query(q): Query<ExportQuery>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_user() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let exports = state.privacy.list_exports(user).await.unwrap_or_default();
    let items: Vec<view::ExportVm> = exports
        .iter()
        .map(|e| {
            let token = if e.id == id { q.token.clone() } else { None };
            view::export_vm(tr, e, token)
        })
        .collect();
    let notice = if q.error.is_some() {
        Some(tr.t("export.error").to_string())
    } else {
        None
    };
    render(
        AccountExportPage {
            layout: PageLayout::with_csrf(
                &state.map,
                tr.t("export.title").to_string(),
                "account",
                auth.csrf_value(),
            ),
            tr,
            items,
            notice,
        },
        StatusCode::OK,
    )
}

/// GET /account/export/{id}/download?token=… — owner-only, single-use, expiring.
async fn account_export_download(
    State(state): State<AppState>,
    _locale: Locale,
    auth: Auth,
    Path(id): Path<i64>,
    Query(q): Query<ExportQuery>,
) -> Response {
    let user = match auth.require_user() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let token = q.token.as_deref().unwrap_or("");
    match state.privacy.download_export(user, id, token).await {
        Ok(download) => {
            let body =
                serde_json::to_vec_pretty(&download.payload).unwrap_or_else(|_| b"{}".to_vec());
            let mut resp = body.into_response();
            resp.headers_mut().insert(
                header::CONTENT_TYPE,
                "application/json; charset=utf-8".parse().unwrap(),
            );
            resp.headers_mut().insert(
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"bikenest-export.json\""
                    .parse()
                    .unwrap(),
            );
            resp.headers_mut().insert(
                header::HeaderName::from_static("x-robots-tag"),
                "noindex".parse().unwrap(),
            );
            resp
        }
        // Uniform response for a non-existent id and an id that belongs to
        // another user, so the endpoint does not leak whether an export id
        // exists (no id-probe oracle).
        Err(PrivacyError::NotFound | PrivacyError::NotAuthorized) => {
            (StatusCode::FORBIDDEN, "Forbidden").into_response()
        }
        // The legitimate "link no longer works" cases (expired / already used /
        // bad token) land back on the owner's C7 page with a notice.
        Err(
            PrivacyError::Expired | PrivacyError::AlreadyDownloaded | PrivacyError::InvalidToken,
        ) => Redirect::to(&format!("/account/export/{id}?error=1")).into_response(),
        Err(_) => Redirect::to(&format!("/account/export/{id}?error=1")).into_response(),
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct DeleteForm {
    email: String,
    #[serde(default)]
    password: String,
}

/// GET /account/delete — deletion confirmation form.
async fn account_delete(State(state): State<AppState>, locale: Locale, auth: Auth) -> Response {
    let tr = Translator::new(locale);
    if let Err(resp) = auth.require_user() {
        return resp;
    }
    render(
        AccountDeletePage {
            layout: PageLayout::with_csrf(
                &state.map,
                tr.t("delete.title").to_string(),
                "account",
                auth.csrf_value(),
            ),
            tr,
            error: None,
        },
        StatusCode::OK,
    )
}

/// POST /account/delete — re-auth + confirm, then anonymize-in-place.
async fn account_delete_post(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    Form(form): Form<DeleteForm>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_user() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let password = if form.password.trim().is_empty() {
        None
    } else {
        Some(form.password.as_str())
    };
    let delete_err_status = |tr: Translator, key: &str, status: StatusCode| {
        render(
            AccountDeletePage {
                layout: PageLayout::with_csrf(
                    &state.map,
                    tr.t("delete.title").to_string(),
                    "account",
                    auth.csrf_value(),
                ),
                tr,
                error: Some(tr.t(key).to_string()),
            },
            status,
        )
    };
    let delete_err = |tr: Translator, key: &str| delete_err_status(tr, key, StatusCode::OK);
    match state
        .privacy
        .request_deletion(user, password, &form.email)
        .await
    {
        Ok(()) => {
            // Session was revoked in the service; clear the cookie too.
            let mut resp = Redirect::to("/login?deleted=1").into_response();
            resp.headers_mut()
                .insert(header::SET_COOKIE, clear_session_cookie().parse().unwrap());
            resp
        }
        Err(PrivacyError::LastAdmin) => delete_err(tr, "delete.last_admin_error"),
        Err(PrivacyError::ReauthRequired) => delete_err(tr, "delete.reauth_error"),
        Err(PrivacyError::Conflict) => {
            delete_err_status(tr, "error.conflict", StatusCode::CONFLICT)
        }
        Err(PrivacyError::Unavailable) => {
            delete_err_status(tr, "error.unavailable", StatusCode::SERVICE_UNAVAILABLE)
        }
        Err(_) => delete_err(tr, "delete.reauth_error"),
    }
}

/// GET /admin/privacy-requests — the manual rights queue (ADMIN-only).
async fn admin_privacy_requests(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
) -> Response {
    let tr = Translator::new(locale);
    let admin = match auth.require_role(Role::Admin) {
        Ok(a) => a,
        Err(resp) => return resp,
    };
    let items: Vec<view::PrivacyRequestVm> = state
        .privacy
        .list_requests(admin, None)
        .await
        .unwrap_or_default()
        .iter()
        .map(|r| view::privacy_request_vm(tr, r))
        .collect();
    render(
        AdminPrivacyRequestsPage {
            layout: PageLayout::with_csrf(
                &state.map,
                tr.t("admin.privacy_requests.title").to_string(),
                "moderation",
                auth.csrf_value(),
            ),
            tr,
            items,
            notice: None,
        },
        StatusCode::OK,
    )
}

/// POST /admin/privacy-requests/{id}/fulfill — mark a manual request COMPLETED.
async fn admin_privacy_request_fulfill(
    State(state): State<AppState>,
    _locale: Locale,
    auth: Auth,
    Path(id): Path<i64>,
) -> Response {
    let admin = match auth.require_role(Role::Admin) {
        Ok(a) => a,
        Err(resp) => return resp,
    };
    match state.privacy.fulfill_request(admin, id).await {
        Ok(()) => Redirect::to("/admin/privacy-requests?fulfilled=1").into_response(),
        Err(_) => Redirect::to("/admin/privacy-requests?error=1").into_response(),
    }
}

fn render<T: Template>(template: T, status: StatusCode) -> Response {
    match template.render() {
        Ok(html) => (status, Html(html)).into_response(),
        // A render failure is a bug; keep the fallback minimal (no template).
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response(),
    }
}

/// Render an anonymous form page that carries a double-submit CSRF token: the
/// token goes into the layout (hidden `csrf` field / `<meta name="csrf">`) and
/// the matching `csrf` cookie is set on the response (see web/auth.rs §108).
fn render_anon<T: Template>(page: T, token: &str) -> Response {
    let mut resp = render(page, StatusCode::OK);
    if let Ok(value) = set_anon_csrf_cookie(token).parse() {
        resp.headers_mut().insert(header::SET_COOKIE, value);
    }
    resp
}

/// A translated failure page. Like every other error path this respects the
/// request's shape: a real htmx fragment request gets
/// `partials/fragment_error.html` (it is swapped into a live target, so a whole
/// document would land inside it), anything else gets the styled `error.html`.
fn error_page(
    headers: &HeaderMap,
    map: &MapConfig,
    tr: Translator,
    status: StatusCode,
    body_key: &str,
) -> Response {
    crate::error_response(headers, map, tr, status, tr.t(body_key).to_string())
}

fn internal_error(headers: &HeaderMap, map: &MapConfig, tr: Translator) -> Response {
    error_page(
        headers,
        map,
        tr,
        StatusCode::INTERNAL_SERVER_ERROR,
        "error.500.body",
    )
}

fn not_found_page(headers: &HeaderMap, map: &MapConfig, tr: Translator) -> Response {
    error_page(headers, map, tr, StatusCode::NOT_FOUND, "error.404.body")
}

/// Router fallback (E1). A 404 is reachable by every kind of request — a typed
/// URL, a boosted link, a stale htmx fragment poll — so it answers each in its
/// own shape rather than always emitting a whole document.
async fn not_found(State(state): State<AppState>, locale: Locale, headers: HeaderMap) -> Response {
    not_found_page(&headers, &state.map, Translator::new(locale))
}

/// Paths whose plain-text bodies are the contract, not a leaked failure: the
/// probes, the media redirector and the static file service.
fn skips_styled_errors(path: &str) -> bool {
    path == "/healthz"
        || path == "/readyz"
        || path == "/robots.txt"
        || path == "/sitemap.xml"
        || path.starts_with("/media/")
        || path.starts_with("/static/")
}

/// True when the caller would understand an HTML answer (no `Accept` at all,
/// or one that admits `text/html` / `*/*`).
fn accepts_html(headers: &HeaderMap) -> bool {
    match headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()) {
        None => true,
        Some(a) => a.contains("text/html") || a.contains("*/*"),
    }
}

/// A translated sentence for a status the router (rather than a handler)
/// produced, so the user never sees axum's English rejection text.
fn status_message(tr: Translator, status: StatusCode) -> String {
    let key = match status {
        StatusCode::UNAUTHORIZED => "error.login_required",
        StatusCode::FORBIDDEN => "error.forbidden",
        StatusCode::NOT_FOUND => "error.404.body",
        StatusCode::METHOD_NOT_ALLOWED => "error.method_not_allowed",
        StatusCode::CONFLICT => "error.conflict",
        StatusCode::PAYLOAD_TOO_LARGE => "error.too_large",
        StatusCode::TOO_MANY_REQUESTS => "error.too_many",
        StatusCode::SERVICE_UNAVAILABLE => "error.unavailable",
        s if s.is_server_error() => "error.500.body",
        _ => "error.bad_request",
    };
    tr.t(key).to_string()
}

/// Last line of defence for E1/E2: any failing response that is still bare text
/// (axum's extractor rejections, the router's 405, a stray literal) is re-rendered
/// as the styled error page — or as `partials/fragment_error.html` when htmx
/// asked for a fragment, because htmx 4 swaps 4xx/5xx bodies too.
async fn styled_errors(
    State(state): State<AppState>,
    req: axum::extract::Request,
    next: middleware::Next,
) -> Response {
    let path = req.uri().path().to_string();
    let headers = req.headers().clone();
    let res = next.run(req).await;

    if res.status().is_success() || res.status().is_redirection() {
        return res;
    }
    if skips_styled_errors(&path) || !accepts_html(&headers) {
        return res;
    }
    let is_plain = match res.headers().get(header::CONTENT_TYPE) {
        None => true,
        Some(ct) => ct
            .to_str()
            .is_ok_and(|ct| ct.starts_with("text/plain") || ct.starts_with("application/octet")),
    };
    if !is_plain {
        return res;
    }
    let status = res.status();
    let tr = Translator::new(Locale::from_headers(&headers));
    let mut styled = crate::error_response(
        &headers,
        &state.map,
        tr,
        status,
        status_message(tr, status),
    );
    // Keep whatever the inner response set (`Allow` on a 405, `Set-Cookie`, …);
    // only the body and its content type are replaced.
    for (name, value) in res.headers() {
        if name != header::CONTENT_TYPE && name != header::CONTENT_LENGTH {
            styled.headers_mut().append(name, value.clone());
        }
    }
    styled
}

// ---------------------------------------------------------------------------
// M3 community handlers
// ---------------------------------------------------------------------------

fn parse_bool(s: &str) -> bool {
    s == "true" || s == "1" || s == "on"
}

fn security_from_form(s: &str) -> Vec<SecurityFeature> {
    s.split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .filter(|c| is_known_security_code(c))
        .map(|c| SecurityFeature::new(c, SecurityState::Yes))
        .collect()
}

/// Parse a price in major units ("5", "5.50", "5,50") into cents.
fn parse_price_major_to_cents(raw: &str) -> Option<i64> {
    let s = raw.trim().replace(',', ".");
    if s.is_empty() {
        return None;
    }
    let major: f64 = s.parse().ok()?;
    if major < 0.0 {
        return None;
    }
    Some((major * 100.0).round() as i64)
}

/// Render a cent amount as a major-units string for the price input (no floats).
fn cents_to_major_string(cents: i64) -> String {
    let major = cents / 100;
    let frac = (cents % 100).abs();
    if frac == 0 {
        major.to_string()
    } else {
        format!("{major}.{frac:02}")
    }
}

fn cost_from_form(form: &NewParkingForm) -> Result<Cost, ContributionError> {
    match form.cost_kind.as_str() {
        "free" => Ok(Cost::Free),
        "paid" => {
            let price = match (
                parse_price_major_to_cents(&form.price),
                &form.price_currency,
                &form.price_unit,
            ) {
                (Some(cents), cur, unit) if !cur.is_empty() && !unit.is_empty() => {
                    let currency = CurrencyCode::parse(cur)
                        .map_err(|e| ContributionError::InvalidField(e.to_string()))?;
                    let unit = PricingUnit::from_code(unit)
                        .map_err(|e| ContributionError::InvalidField(e.to_string()))?;
                    Some(Money::new(cents, currency, unit))
                }
                _ => None,
            };
            Ok(Cost::Paid { price })
        }
        _ => Ok(Cost::Unknown),
    }
}

fn new_location_from_form(form: &NewParkingForm) -> Result<NewParkingLocation, ContributionError> {
    let parking_type = ParkingType::from_code(&form.parking_type)
        .map_err(|e| ContributionError::InvalidField(e.to_string()))?;
    let cost = cost_from_form(form)?;
    let lat = form
        .lat
        .trim()
        .parse::<f64>()
        .map_err(|_| ContributionError::InvalidField("latitude is required".to_string()))?;
    let lon = form
        .lon
        .trim()
        .parse::<f64>()
        .map_err(|_| ContributionError::InvalidField("longitude is required".to_string()))?;
    let point =
        GeoPoint::new(lat, lon).map_err(|e| ContributionError::InvalidField(e.to_string()))?;
    let timezone = if form.timezone.trim().is_empty() {
        None
    } else {
        Some(
            form.timezone
                .trim()
                .parse()
                .map_err(|_| ContributionError::InvalidField("invalid timezone".to_string()))?,
        )
    };
    let hours = if parse_bool(&form.open_24h) {
        OpeningHours::weekly((1..=7).map(|d| (d, TimeRange::all_day())).collect())
    } else {
        OpeningHours::Unknown
    };
    let description = if form.description.trim().is_empty() {
        None
    } else {
        Some(form.description.trim().to_string())
    };
    Ok(NewParkingLocation {
        name: form.name.clone(),
        address: form.address.clone(),
        description,
        parking_type,
        cost,
        point,
        timezone,
        hours,
        security: security_from_form(&form.security),
    })
}

fn edit_from_form(
    form: &EditParkingForm,
    current_hours: &OpeningHours,
) -> Result<ParkingEdit, ContributionError> {
    let parking_type = ParkingType::from_code(&form.parking_type)
        .map_err(|e| ContributionError::InvalidField(e.to_string()))?;
    let cost = cost_from_form(&NewParkingForm {
        cost_kind: form.cost_kind.clone(),
        price: form.price.clone(),
        price_currency: form.price_currency.clone(),
        price_unit: form.price_unit.clone(),
        ..Default::default()
    })?;
    // Preserve the original hours unless the user explicitly toggled the 24h
    // switch — otherwise submitting an unrelated field would wipe real hours.
    let current_24h = hours_open_24h(current_hours);
    let submitted_24h = parse_bool(&form.open_24h);
    let hours = if submitted_24h == current_24h {
        current_hours.clone()
    } else if submitted_24h {
        OpeningHours::weekly((1..=7).map(|d| (d, TimeRange::all_day())).collect())
    } else {
        OpeningHours::Unknown
    };
    let description = if form.description.trim().is_empty() {
        None
    } else {
        Some(form.description.trim().to_string())
    };
    Ok(ParkingEdit {
        name: form.name.clone(),
        address: form.address.clone(),
        description,
        parking_type,
        cost,
        hours,
        security: security_from_form(&form.security),
    })
}

/// The form's `cost_kind` value for a location (used to pre-fill the edit form).
fn cost_kind_string(cost: &Cost) -> String {
    match cost {
        Cost::Free => "free",
        Cost::Paid { .. } => "paid",
        Cost::Unknown => "unknown",
    }
    .to_string()
}

/// `(major-price, currency, unit)` as form strings, for pre-filling a paid
/// price. The user types a human-readable amount ("R$ 5"); the backend stores
/// cents — so we pre-fill in major units, not cents.
fn cost_price_strings(cost: &Cost) -> (String, String, String) {
    match cost {
        Cost::Paid { price: Some(p) } => (
            cents_to_major_string(p.cents()),
            p.currency().as_str().to_string(),
            p.unit().as_code().to_string(),
        ),
        _ => (String::new(), String::new(), String::new()),
    }
}

/// True when the location is open 24h every day (the only "hours" state the
/// add/edit form can express besides unknown).
fn hours_open_24h(hours: &OpeningHours) -> bool {
    matches!(hours, OpeningHours::Weekly(rows) if !rows.is_empty() && rows.iter().all(|(_, r)| r.all_day))
}

/// Comma-separated codes of the security attributes confirmed `yes` (to
/// pre-fill the add/edit checkboxes).
fn security_yes_codes_string(loc: &ParkingLocation) -> String {
    loc.security()
        .iter()
        .filter(|f| f.state() == SecurityState::Yes)
        .map(|f| f.code())
        .collect::<Vec<_>>()
        .join(",")
}

/// Build a `ParkingEditPage` with all reversible fields pre-filled from `loc`.
/// The page chrome shared by every render of the "edit parking" form.
fn edit_parking_layout(map: &MapConfig, tr: Translator, auth: &Auth) -> PageLayout {
    PageLayout::with_csrf(
        map,
        tr.t("edit.title").to_string(),
        "edit",
        auth.csrf_value(),
    )
}

/// The page chrome shared by every render of the "add parking" form.
fn new_parking_layout(map: &MapConfig, tr: Translator, auth: &Auth) -> PageLayout {
    PageLayout::with_csrf(map, tr.t("new.title").to_string(), "new", auth.csrf_value())
}

fn parking_edit_page_vm(
    layout: PageLayout,
    tr: Translator,
    id: i64,
    version: i64,
    loc: &ParkingLocation,
    notice: Option<String>,
    error: Option<String>,
) -> ParkingEditPage {
    let (price, price_currency, price_unit) = cost_price_strings(loc.cost());
    ParkingEditPage {
        layout,
        tr,
        id,
        version,
        name: loc.name().to_string(),
        address: loc.address().to_string(),
        description: loc.description().unwrap_or("").to_string(),
        parking_type: loc.parking_type().as_code().to_string(),
        cost_kind: cost_kind_string(loc.cost()),
        price,
        price_currency,
        price_unit,
        open_24h: hours_open_24h(loc.hours()),
        type_options: view::type_options(tr, Some(loc.parking_type().as_code())),
        security_options: view::security_options(tr, Some(&security_yes_codes_string(loc))),
        security: security_yes_codes_string(loc),
        error,
        notice,
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct NewParkingForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    address: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    parking_type: String,
    #[serde(default)]
    cost_kind: String,
    /// Price in major units (e.g. "5"/"5.50"), NOT cents — cents is a backend
    /// detail. The user types a human-readable amount (see the form UX).
    #[serde(default)]
    price: String,
    #[serde(default)]
    price_currency: String,
    #[serde(default)]
    price_unit: String,
    #[serde(default)]
    lat: String,
    #[serde(default)]
    lon: String,
    #[serde(default)]
    timezone: String,
    #[serde(default)]
    open_24h: String,
    /// Comma-separated security attribute codes, produced by the checkboxes via
    /// a single hidden field (serde_urlencoded rejects repeated keys).
    #[serde(default)]
    security: String,
}

async fn parking_new_page(State(state): State<AppState>, locale: Locale, auth: Auth) -> Response {
    let tr = Translator::new(locale);
    if let Err(resp) = auth.require_verified() {
        return resp;
    }
    render(
        ParkingNewPage {
            layout: PageLayout::with_csrf(
                &state.map,
                tr.t("new.title").to_string(),
                "new",
                auth.csrf_value(),
            ),
            tr,
            name: String::new(),
            address: String::new(),
            description: String::new(),
            parking_type: "rack".to_string(),
            cost_kind: "unknown".to_string(),
            price: String::new(),
            price_currency: String::new(),
            price_unit: String::new(),
            lat: String::new(),
            lon: String::new(),
            timezone: String::new(),
            open_24h: false,
            type_options: view::type_options(tr, None),
            security_options: view::security_options(tr, None),
            security: String::new(),
            error: None,
            duplicates: Vec::new(),
            added_id: None,
        },
        StatusCode::OK,
    )
}

#[allow(clippy::too_many_arguments)]
async fn parking_new_post(
    State(state): State<AppState>,
    locale: Locale,
    ClientIp(ip): ClientIp,
    auth: Auth,
    Form(form): Form<NewParkingForm>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_verified() {
        Ok(u) => u,
        Err(resp) => return resp,
    };

    let new = match new_location_from_form(&form) {
        Ok(n) => n,
        Err(e) => {
            return render_form_error(&state.map, tr, auth, &form, &e);
        }
    };

    match state
        .contributions
        .add_parking_location(user, &ip, new)
        .await
    {
        Ok(outcome) => {
            let duplicates: Vec<view::DuplicateVm> = outcome
                .duplicates
                .iter()
                .map(|d| view::duplicate_vm(tr, d))
                .collect();
            if duplicates.is_empty() {
                axum::response::Redirect::to(&format!("/parking/{}", outcome.id)).into_response()
            } else {
                // Advisory (§36): the location was added, but similar listings
                // exist. Re-render the form with the warnings + a success note.
                render_new_form(
                    new_parking_layout(&state.map, tr, &auth),
                    tr,
                    &form,
                    None,
                    duplicates,
                    Some(outcome.id),
                    StatusCode::OK,
                )
            }
        }
        Err(ContributionError::NotVerified) => {
            axum::response::Redirect::to("/account?verify=1").into_response()
        }
        Err(ContributionError::RateLimited) => render_new_form(
            new_parking_layout(&state.map, tr, &auth),
            tr,
            &form,
            Some(tr.t("contribution.error.rate_limited").to_string()),
            Vec::new(),
            None,
            StatusCode::TOO_MANY_REQUESTS,
        ),
        Err(e) => render_form_error(&state.map, tr, auth, &form, &e),
    }
}

fn render_new_form(
    layout: PageLayout,
    tr: Translator,
    form: &NewParkingForm,
    error: Option<String>,
    duplicates: Vec<view::DuplicateVm>,
    added_id: Option<i64>,
    status: StatusCode,
) -> Response {
    render(
        ParkingNewPage {
            layout,
            tr,
            name: form.name.clone(),
            address: form.address.clone(),
            description: form.description.clone(),
            parking_type: form.parking_type.clone(),
            cost_kind: form.cost_kind.clone(),
            price: form.price.clone(),
            price_currency: form.price_currency.clone(),
            price_unit: form.price_unit.clone(),
            lat: form.lat.clone(),
            lon: form.lon.clone(),
            timezone: form.timezone.clone(),
            open_24h: parse_bool(&form.open_24h),
            type_options: view::type_options(tr, Some(&form.parking_type)),
            security_options: view::security_options(tr, Some(&form.security)),
            security: form.security.clone(),
            error,
            duplicates,
            added_id,
        },
        status,
    )
}

fn render_form_error(
    map: &MapConfig,
    tr: Translator,
    auth: Auth,
    form: &NewParkingForm,
    e: &ContributionError,
) -> Response {
    render_new_form(
        new_parking_layout(map, tr, &auth),
        tr,
        form,
        Some(contribution_error_message(tr, e)),
        Vec::new(),
        None,
        contribution_error_status(e, StatusCode::BAD_REQUEST),
    )
}

fn contribution_error_message(tr: Translator, e: &ContributionError) -> String {
    match e {
        ContributionError::NotVerified => tr.t("contribution.error.not_verified").to_string(),
        ContributionError::RateLimited => tr.t("contribution.error.rate_limited").to_string(),
        ContributionError::VersionConflict => {
            tr.t("contribution.error.version_conflict").to_string()
        }
        ContributionError::NotFound => tr.t("contribution.error.not_found").to_string(),
        ContributionError::LocationNotActive => tr.t("contribution.error.not_active").to_string(),
        ContributionError::InvalidField(_) => tr.t("contribution.error.invalid").to_string(),
        ContributionError::Unauthorized => tr.t("contribution.error.unauthorized").to_string(),
        ContributionError::Timezone => tr.t("contribution.error.timezone").to_string(),
        ContributionError::Conflict => tr.t("error.conflict").to_string(),
        ContributionError::Unavailable => tr.t("error.unavailable").to_string(),
        ContributionError::Internal => tr.t("contribution.error.internal").to_string(),
    }
}

/// Status for a re-rendered contribution form. The variants with a status of
/// their own say so here; every other variant keeps whatever the calling flow
/// already chose (a form re-render is a 400 or a 200).
fn contribution_error_status(e: &ContributionError, default: StatusCode) -> StatusCode {
    match e {
        ContributionError::Conflict => StatusCode::CONFLICT,
        ContributionError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        // A location moderation took down reads as "gone" everywhere, exactly
        // as the public details page already treats it.
        ContributionError::LocationNotActive => StatusCode::NOT_FOUND,
        _ => default,
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct EditParkingForm {
    #[serde(default)]
    version: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    address: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    parking_type: String,
    #[serde(default)]
    cost_kind: String,
    /// Price in major units (e.g. "5"/"5.50"), NOT cents.
    #[serde(default)]
    price: String,
    #[serde(default)]
    price_currency: String,
    #[serde(default)]
    price_unit: String,
    #[serde(default)]
    open_24h: String,
    #[serde(default)]
    security: String,
}

async fn parking_edit_page(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let tr = Translator::new(locale);
    if let Err(resp) = auth.require_verified() {
        return resp;
    }
    let Some(view) = state.details.execute(id).await.ok().flatten() else {
        return not_found_page(&headers, &state.map, tr);
    };
    // A location moderation took down accepts no contributions, so the form
    // (and the sensitive-change proposals it hosts) is gone for everyone —
    // moderators included, since the write path refuses them too.
    if view.location.moderation_state() != ModerationState::Active {
        return not_found_page(&headers, &state.map, tr);
    }
    let loc = &view.location;
    // Pre-fill every reversible field so editing one doesn't silently reset
    // cost/security/hours (§7 "editable fields pre-filled").
    render(
        parking_edit_page_vm(
            edit_parking_layout(&state.map, tr, &auth),
            tr,
            id,
            loc.version(),
            loc,
            None,
            None,
        ),
        StatusCode::OK,
    )
}

async fn parking_edit_post(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<EditParkingForm>,
) -> Response {
    let tr = Translator::new(locale);
    if let Err(resp) = auth.require_verified() {
        return resp;
    }
    let user = match auth.require_verified() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    // Load the current location so an untouched field is preserved (and so we
    // can detect a version conflict against the latest values).
    let current = match state.details.execute(id).await {
        Ok(Some(v)) => v.location,
        _ => return not_found_page(&headers, &state.map, tr),
    };
    let current_hours = current.hours().clone();
    let edit = match edit_from_form(&form, &current_hours) {
        Ok(e) => e,
        Err(e) => return contribution_edit_error(&state.map, tr, auth, id, &form, &e),
    };
    match state
        .contributions
        .apply_parking_edit(user, id, form.version, &edit)
        .await
    {
        Ok(_) => axum::response::Redirect::to(&format!("/parking/{id}?edited=1")).into_response(),
        Err(ContributionError::VersionConflict) => {
            // §100: reload the latest values and tell the user.
            let Some(view) = state.details.execute(id).await.ok().flatten() else {
                return not_found_page(&headers, &state.map, tr);
            };
            let loc = view.location;
            render(
                parking_edit_page_vm(
                    edit_parking_layout(&state.map, tr, &auth),
                    tr,
                    id,
                    loc.version(),
                    &loc,
                    Some(tr.t("contribution.error.version_conflict").to_string()),
                    None,
                ),
                StatusCode::OK,
            )
        }
        Err(ContributionError::RateLimited) => contribution_edit_notice(
            &state.map,
            tr,
            auth,
            id,
            &form,
            tr.t("contribution.error.rate_limited").to_string(),
        ),
        // Same answer as the GET above: there is nothing here to edit.
        Err(ContributionError::LocationNotActive) => not_found_page(&headers, &state.map, tr),
        Err(e) => contribution_edit_error(&state.map, tr, auth, id, &form, &e),
    }
}

fn contribution_edit_error(
    map: &MapConfig,
    tr: Translator,
    auth: Auth,
    id: i64,
    form: &EditParkingForm,
    e: &ContributionError,
) -> Response {
    contribution_edit_notice_status(
        map,
        tr,
        auth,
        id,
        form,
        contribution_error_message(tr, e),
        contribution_error_status(e, StatusCode::OK),
    )
}

fn contribution_edit_notice(
    map: &MapConfig,
    tr: Translator,
    auth: Auth,
    id: i64,
    form: &EditParkingForm,
    notice: String,
) -> Response {
    contribution_edit_notice_status(map, tr, auth, id, form, notice, StatusCode::OK)
}

fn contribution_edit_notice_status(
    map: &MapConfig,
    tr: Translator,
    auth: Auth,
    id: i64,
    form: &EditParkingForm,
    notice: String,
    status: StatusCode,
) -> Response {
    render(
        ParkingEditPage {
            layout: PageLayout::with_csrf(
                map,
                tr.t("edit.title").to_string(),
                "edit",
                auth.csrf_value(),
            ),
            tr,
            id,
            version: form.version,
            name: form.name.clone(),
            address: form.address.clone(),
            description: form.description.clone(),
            parking_type: form.parking_type.clone(),
            cost_kind: form.cost_kind.clone(),
            price: form.price.clone(),
            price_currency: form.price_currency.clone(),
            price_unit: form.price_unit.clone(),
            open_24h: parse_bool(&form.open_24h),
            type_options: view::type_options(tr, Some(&form.parking_type)),
            security_options: view::security_options(tr, Some(&form.security)),
            security: form.security.clone(),
            error: None,
            notice: Some(notice),
        },
        status,
    )
}

#[derive(Debug, Default, serde::Deserialize)]
struct ProposalForm {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    lat: String,
    #[serde(default)]
    lon: String,
    #[serde(default)]
    timezone: String,
    #[serde(default)]
    existence: String,
    #[serde(default)]
    reason: String,
}

async fn parking_proposal_post(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<ProposalForm>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_verified() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let kind = match ProposalKind::from_code(&form.kind) {
        Ok(k) => k,
        Err(_) => return axum::response::Redirect::to(&format!("/parking/{id}")).into_response(),
    };
    let proposed = match kind {
        ProposalKind::MoveLocation => {
            let lat = form.lat.trim().parse::<f64>().unwrap_or(0.0);
            let lon = form.lon.trim().parse::<f64>().unwrap_or(0.0);
            let tz = if form.timezone.trim().is_empty() {
                "America/Sao_Paulo"
            } else {
                form.timezone.as_str()
            };
            serde_json::json!({ "lat": lat, "lon": lon, "timezone": tz, "reason": form.reason })
        }
        ProposalKind::ChangeExistence => {
            serde_json::json!({ "existence": form.existence, "reason": form.reason })
        }
    };
    match state
        .contributions
        .propose_location_change(user, id, kind, proposed)
        .await
    {
        Ok(_) => axum::response::Redirect::to(&format!("/parking/{id}?proposed=1")).into_response(),
        // The proposal forms live on the edit page, which 404s for a
        // taken-down location; a direct POST gets the same answer rather than
        // a redirect to a page that would itself 404.
        Err(ContributionError::LocationNotActive) => not_found_page(&headers, &state.map, tr),
        Err(_) => {
            axum::response::Redirect::to(&format!("/parking/{id}?proposal_error=1")).into_response()
        }
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct ReviewForm {
    #[serde(default)]
    rating: u8,
    #[serde(default)]
    body: String,
}

async fn review_page(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    Path(id): Path<i64>,
) -> Response {
    let tr = Translator::new(locale);
    if let Err(resp) = auth.require_verified() {
        return resp;
    }
    let own = state
        .contributions
        .community_details(id, auth.user.as_ref().map(|u| u.id))
        .await
        .ok()
        .flatten()
        .and_then(|c| c.own_review);
    render(
        ReviewFormPage {
            layout: PageLayout::with_csrf(
                &state.map,
                tr.t("review.title").to_string(),
                "review",
                auth.csrf_value(),
            ),
            tr,
            id,
            rating: own.as_ref().map(|r| r.rating.value()).unwrap_or(0),
            body: own.map(|r| r.body.as_str().to_string()).unwrap_or_default(),
            error: None,
        },
        StatusCode::OK,
    )
}

async fn review_post(
    State(state): State<AppState>,
    locale: Locale,
    ClientIp(ip): ClientIp,
    auth: Auth,
    Path(id): Path<i64>,
    mut multipart: Multipart,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_verified() {
        Ok(u) => u,
        Err(resp) => return resp,
    };

    // Multipart form (D3 now carries 0..N photos, §38). Gather text fields, then
    // any uploaded `photo` files. The text publishes immediately; photos hold PENDING_REVIEW.
    let mut rating_u8 = 0u8;
    let mut body = String::new();
    let mut photos: Vec<Vec<u8>> = Vec::new();
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(_) => {
                return render_review_error(
                    &state.map,
                    tr,
                    auth,
                    id,
                    ReviewForm {
                        rating: rating_u8,
                        body,
                    },
                    tr.t("review.error.generic").to_string(),
                );
            }
        };
        match field.name().unwrap_or("") {
            "rating" => {
                if let Ok(text) = field.text().await {
                    rating_u8 = text.trim().parse().unwrap_or(0);
                }
            }
            "body" => {
                if let Ok(text) = field.text().await {
                    body = text;
                }
            }
            "photo" => {
                if let Ok(bytes) = field.bytes().await {
                    photos.push(bytes.to_vec());
                }
            }
            _ => {
                let _ = field.bytes().await;
            }
        }
    }

    let rating = match StarRating::new(rating_u8) {
        Ok(r) => r,
        Err(_) => {
            return render_review_error(
                &state.map,
                tr,
                auth,
                id,
                ReviewForm {
                    rating: rating_u8,
                    body,
                },
                tr.t("review.error.invalid").to_string(),
            );
        }
    };
    let review_body = match ReviewBody::new(&body) {
        Ok(b) => b,
        Err(_) => {
            return render_review_error(
                &state.map,
                tr,
                auth,
                id,
                ReviewForm {
                    rating: rating_u8,
                    body,
                },
                tr.t("review.error.length").to_string(),
            );
        }
    };
    match state
        .contributions
        .upsert_review(user, id, rating, &review_body)
        .await
    {
        Ok(()) => {
            // Attach any uploaded photos to the (just-upserted) review, held PENDING_REVIEW.
            if !photos.is_empty()
                && let Ok(Some(own)) = state
                    .contributions
                    .community_details(id, Some(user.id))
                    .await
                && let Some(review) = own.own_review
            {
                for p in photos {
                    let _ = state
                        .photo
                        .upload_photo(user, &ip, PhotoTarget::Review(review.id), &p, None)
                        .await;
                }
            }
            axum::response::Redirect::to(&format!("/parking/{id}?reviewed=1")).into_response()
        }
        Err(ContributionError::RateLimited) => render_review_error(
            &state.map,
            tr,
            auth,
            id,
            ReviewForm {
                rating: rating_u8,
                body,
            },
            tr.t("contribution.error.rate_limited").to_string(),
        ),
        // A duplicate review or a lost race is the user's to resolve (409); a
        // spot that no longer takes contributions is gone (404); an unreachable
        // database is ours (503). Everything else stays generic.
        Err(
            e @ (ContributionError::Conflict
            | ContributionError::Unavailable
            | ContributionError::LocationNotActive),
        ) => {
            render_review_error_status(
                &state.map,
                tr,
                auth,
                id,
                ReviewForm {
                    rating: rating_u8,
                    body,
                },
                contribution_error_message(tr, &e),
                contribution_error_status(&e, StatusCode::OK),
            )
        }
        Err(_) => render_review_error(
            &state.map,
            tr,
            auth,
            id,
            ReviewForm {
                rating: rating_u8,
                body,
            },
            tr.t("review.error.generic").to_string(),
        ),
    }
}

fn render_review_error(
    map: &MapConfig,
    tr: Translator,
    auth: Auth,
    id: i64,
    form: ReviewForm,
    message: String,
) -> Response {
    render_review_error_status(map, tr, auth, id, form, message, StatusCode::OK)
}

fn render_review_error_status(
    map: &MapConfig,
    tr: Translator,
    auth: Auth,
    id: i64,
    form: ReviewForm,
    message: String,
    status: StatusCode,
) -> Response {
    render(
        ReviewFormPage {
            layout: PageLayout::with_csrf(
                map,
                tr.t("review.title").to_string(),
                "review",
                auth.csrf_value(),
            ),
            tr,
            id,
            rating: form.rating,
            body: form.body,
            error: Some(message),
        },
        status,
    )
}

#[derive(Debug, Default, serde::Deserialize)]
struct VerifyForm {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    result: String,
    #[serde(default)]
    attribute_code: String,
}

async fn parking_verify_post(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<VerifyForm>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_verified() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let ctx = VerifyCtx {
        headers: &headers,
        map: &state.map,
        tr,
        back: format!("/parking/{id}?verified=1"),
    };
    // Validate the submitted kind/result/attribute (§39) rather than silently
    // coercing unknown inputs into StillExists/Correct.
    let signal = match form.kind.as_str() {
        "attribute" => {
            let result = match form.result.as_str() {
                "correct" => bikenest_domain::AttributeResult::Correct,
                "incorrect" => bikenest_domain::AttributeResult::Incorrect,
                _ => {
                    return verify_bad_request(&ctx);
                }
            };
            if !is_known_attribute_code(&form.attribute_code) {
                return verify_bad_request(&ctx);
            }
            NewVerification::Attribute {
                location_id: id,
                user_id: user.id,
                code: form.attribute_code.clone(),
                result,
            }
        }
        "parked_here" => NewVerification::ParkedHere {
            location_id: id,
            user_id: user.id,
        },
        "existence" => {
            let result = match form.result.as_str() {
                "still_exists" => ExistenceResult::StillExists,
                "no_longer_exists" => ExistenceResult::NoLongerExists,
                "info_changed" => ExistenceResult::InfoChanged,
                _ => return verify_bad_request(&ctx),
            };
            NewVerification::Existence {
                location_id: id,
                user_id: user.id,
                result,
            }
        }
        _ => return verify_bad_request(&ctx),
    };
    match state.contributions.record_verification(user, &signal).await {
        Ok(()) => verification_saved(&ctx, tr.t("verification.saved")),
        Err(e) => verification_error(&ctx, &e),
    }
}

/// What the small P3 verification fragments need to answer both callers: the
/// request's htmx headers, the map config for the styled error page, and the
/// page a whole-document request is redirected back to.
struct VerifyCtx<'a> {
    headers: &'a HeaderMap,
    map: &'a MapConfig,
    tr: Translator,
    back: String,
}

fn verification_result(
    ctx: &VerifyCtx<'_>,
    state: &'static str,
    label: &str,
    status: StatusCode,
) -> Response {
    let tr = ctx.tr;
    fragment_answer(ctx.headers, ctx.map, tr, status, label, &ctx.back, || {
        render(
            crate::VerificationResultVm {
                tr,
                state,
                label: label.to_string(),
            },
            status,
        )
    })
}

fn verification_saved(ctx: &VerifyCtx<'_>, label: &str) -> Response {
    verification_result(ctx, "success", label, StatusCode::OK)
}

fn verify_bad_request(ctx: &VerifyCtx<'_>) -> Response {
    verification_result(
        ctx,
        "error",
        ctx.tr.t("contribution.error.invalid"),
        StatusCode::BAD_REQUEST,
    )
}

/// Error state of the verification fragment. A lost race (409) and an
/// unreachable database (503) say so; every other variant keeps the generic
/// message and the 400 these endpoints have always returned.
fn verification_error(ctx: &VerifyCtx<'_>, e: &ContributionError) -> Response {
    verification_result(
        ctx,
        "error",
        &contribution_fragment_message(ctx.tr, e),
        contribution_error_status(e, StatusCode::BAD_REQUEST),
    )
}

/// Message for the small htmx fragments the P3 detail page swaps in. Only the
/// variants a user can act on get their own copy; anything else stays
/// deliberately vague.
fn contribution_fragment_message(tr: Translator, e: &ContributionError) -> String {
    match e {
        ContributionError::Conflict
        | ContributionError::Unavailable
        | ContributionError::LocationNotActive => contribution_error_message(tr, e),
        _ => tr.t("contribution.error.generic").to_string(),
    }
}

async fn parking_parked_here_post(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_verified() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let ctx = VerifyCtx {
        headers: &headers,
        map: &state.map,
        tr,
        back: format!("/parking/{id}?parked=1"),
    };
    let signal = NewVerification::ParkedHere {
        location_id: id,
        user_id: user.id,
    };
    match state.contributions.record_verification(user, &signal).await {
        Ok(()) => verification_saved(&ctx, tr.t("parked.saved")),
        Err(e) => verification_error(&ctx, &e),
    }
}

async fn parking_favorite_post(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_user() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    // The success response *is* the button, so a whole-document caller (no JS,
    // or a boosted submit) is sent back to the page, which re-renders it in the
    // new state — no notice flag needed.
    let back = format!("/parking/{id}");
    match state.contributions.toggle_favorite(user.id, id).await {
        Ok(is_favorited) => fragment_or_redirect(
            &headers,
            render(
                crate::FavoriteButtonVm {
                    tr,
                    id,
                    is_favorited,
                    csrf: auth.csrf_value(),
                },
                StatusCode::OK,
            ),
            &back,
        ),
        // The error swap needs a fragment of its own. 409/503 for the mapper's
        // variants; everything else keeps the 500 it returned before, now with
        // translated copy.
        Err(e) => {
            let message = contribution_fragment_message(tr, &e);
            let status = contribution_error_status(&e, StatusCode::INTERNAL_SERVER_ERROR);
            fragment_answer(&headers, &state.map, tr, status, &message, &back, || {
                render(
                    crate::FragmentErrorVm {
                        tr,
                        message: message.clone(),
                    },
                    status,
                )
            })
        }
    }
}

async fn account_favorites(State(state): State<AppState>, locale: Locale, auth: Auth) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_user() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let ids = state
        .contributions
        .list_favorites(user.id)
        .await
        .unwrap_or_default();
    let now = chrono::Utc::now();
    let mut items = Vec::new();
    for tid in ids {
        // Read each favorite as a summary card (best-effort; skip missing).
        if let Some(view) = state.details.execute(tid).await.ok().flatten() {
            let loc = &view.location;
            let summary = bikenest_application::ParkingSummary {
                id: loc.id(),
                name: loc.name().to_string(),
                address: loc.address().to_string(),
                parking_type: loc.parking_type(),
                cost: loc.cost().clone(),
                point: *loc.point(),
                distance_m: 0.0,
                security_yes: loc
                    .security()
                    .iter()
                    .filter(|f| f.state() == SecurityState::Yes)
                    .map(|f| f.code().to_string())
                    .collect(),
                rating: *loc.rating(),
                last_verified_at: loc.last_verified_at(),
                timezone: loc.timezone(),
                is_open_now: loc.hours().status_at(now, loc.timezone())
                    == bikenest_domain::OpenStatus::Open,
                photo_key: None,
                // Favorites are listed whole, not paginated by keyset.
                sort_key: None,
            };
            let freshness = bikenest_domain::categorize(
                loc.last_verified_at(),
                now,
                &state.freshness.thresholds,
            );
            let photo_url = view::resolve_photo(&*state.storage, None).await;
            items.push(CardVm::from_summary(tr, &summary, freshness, photo_url));
        }
    }
    render(
        FavoritesPage {
            layout: PageLayout::with_csrf(
                &state.map,
                tr.t("favorites.title").to_string(),
                "account",
                auth.csrf_value(),
            ),
            tr,
            items,
            notice: None,
        },
        StatusCode::OK,
    )
}

async fn account_contributions(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_user() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let items = state
        .contributions
        .contribution_history(user.id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|i| view::contribution_vm(tr, &i))
        .collect();
    render(
        ContributionsPage {
            layout: PageLayout::with_csrf(
                &state.map,
                tr.t("contrib.title").to_string(),
                "account",
                auth.csrf_value(),
            ),
            tr,
            items,
        },
        StatusCode::OK,
    )
}

// ---------------------------------------------------------------------------
// Error-mapping unit tests
// ---------------------------------------------------------------------------
//
// The handlers that surface these are covered end to end in
// `tests/http_test.rs`; provoking a real database conflict through HTTP is
// inherently racy, so the mapping itself is pinned here instead.

#[cfg(test)]
mod tests {
    use super::*;

    fn en() -> Translator {
        Translator::new(Locale::En)
    }

    #[test]
    fn moderation_conflict_is_409_and_unavailable_is_503() {
        let (status, message) = moderation_error_message(en(), &ModerationError::Conflict);
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(message, en().t("error.conflict"));

        let (status, message) = moderation_error_message(en(), &ModerationError::Unavailable);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(message, en().t("error.unavailable"));
    }

    #[test]
    fn photo_conflict_is_409_and_unavailable_is_503() {
        let (status, message) = photo_error(en(), &PhotoError::Conflict);
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(message, en().t("error.conflict"));

        let (status, message) = photo_error(en(), &PhotoError::Unavailable);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(message, en().t("error.unavailable"));
    }

    #[test]
    fn contribution_conflict_is_409_and_unavailable_is_503() {
        assert_eq!(
            contribution_error_status(&ContributionError::Conflict, StatusCode::BAD_REQUEST),
            StatusCode::CONFLICT
        );
        assert_eq!(
            contribution_error_status(&ContributionError::Unavailable, StatusCode::OK),
            StatusCode::SERVICE_UNAVAILABLE
        );
        // Every other variant keeps the status its own flow chose.
        assert_eq!(
            contribution_error_status(&ContributionError::NotFound, StatusCode::BAD_REQUEST),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            contribution_error_message(en(), &ContributionError::Conflict),
            en().t("error.conflict")
        );
        assert_eq!(
            contribution_error_message(en(), &ContributionError::Unavailable),
            en().t("error.unavailable")
        );
    }

    /// The three htmx endpoints on the P3 detail page (verify, parked-here,
    /// favorite) used to collapse every error into a generic 400 or a bare
    /// "Internal" 500.
    #[test]
    fn detail_page_fragments_surface_conflict_409_and_unavailable_503() {
        // Verify / parked-here: 400 is the fallback for every other variant.
        assert_eq!(
            contribution_error_status(&ContributionError::Conflict, StatusCode::BAD_REQUEST),
            StatusCode::CONFLICT
        );
        assert_eq!(
            contribution_error_status(&ContributionError::Unavailable, StatusCode::BAD_REQUEST),
            StatusCode::SERVICE_UNAVAILABLE
        );
        // Favorite: 500 is the fallback, and it is no longer a bare string.
        assert_eq!(
            contribution_error_status(
                &ContributionError::Conflict,
                StatusCode::INTERNAL_SERVER_ERROR
            ),
            StatusCode::CONFLICT
        );
        assert_eq!(
            contribution_error_status(
                &ContributionError::Unavailable,
                StatusCode::INTERNAL_SERVER_ERROR
            ),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            contribution_error_status(&ContributionError::Internal, StatusCode::BAD_REQUEST),
            StatusCode::BAD_REQUEST
        );

        // The fragments carry the translated copy, not a bare English string.
        assert_eq!(
            contribution_fragment_message(en(), &ContributionError::Conflict),
            en().t("error.conflict")
        );
        assert_eq!(
            contribution_fragment_message(en(), &ContributionError::Unavailable),
            en().t("error.unavailable")
        );
        assert_eq!(
            contribution_fragment_message(en(), &ContributionError::Internal),
            en().t("contribution.error.generic")
        );
    }

    #[test]
    fn auth_conflict_and_unavailable_get_their_own_copy() {
        assert_eq!(
            auth_error_message(en(), &AuthError::Conflict),
            en().t("error.conflict")
        );
        assert_eq!(
            auth_error_message(en(), &AuthError::Unavailable),
            en().t("error.unavailable")
        );
    }
}
