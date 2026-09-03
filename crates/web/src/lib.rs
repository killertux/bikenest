//! BikeNest web crate: axum routing, handlers, Askama templates.

pub mod auth;
pub mod http;
pub mod i18n;
pub mod observability;
pub mod security;
pub mod view;

use askama::Template;
use bikenest_application::ParkingDetailsView;
use i18n::Translator;

/// Base layout data shared by all pages. `current` drives the active nav item;
/// `csrf` is the per-session synchronizer token (empty when anonymous) — rendered
/// into the `<meta name="csrf">` tag the CSRF middleware / htmx reads.
pub struct PageLayout {
    pub title: String,
    pub current: String,
    pub csrf: String,
    /// Canonical URL (§111) — rendered as `<link rel="canonical">` + `og:url`.
    pub canonical: String,
    /// Meta description + `og:description` (short, localised).
    pub description: String,
    /// OpenGraph type: "website" (default) or "article".
    pub og_type: &'static str,
    /// Map style URL (Ledger #3); rendered onto `<body>` data attributes so the
    /// map JS (search.js / details-map.js) reads it, CSP-safe (no inline script).
    pub map_style_url: String,
    /// Public Mapbox access token for the style/tiles; empty for a non-Mapbox
    /// style (e.g. demo tiles) so the token never lands on the page.
    pub map_access_token: String,
}

impl PageLayout {
    /// A public page layout (no CSRF token).
    pub fn new(title: String, current: &str) -> Self {
        let map = bikenest_infrastructure::map_config_from_env();
        Self {
            title,
            current: current.to_string(),
            csrf: String::new(),
            canonical: String::new(),
            description: String::new(),
            og_type: "website",
            map_style_url: map.style_url,
            map_access_token: map.access_token,
        }
    }

    /// A layout carrying the session's CSRF token (for authenticated forms).
    pub fn with_csrf(title: String, current: &str, csrf: String) -> Self {
        let map = bikenest_infrastructure::map_config_from_env();
        Self {
            title,
            current: current.to_string(),
            csrf,
            canonical: String::new(),
            description: String::new(),
            og_type: "website",
            map_style_url: map.style_url,
            map_access_token: map.access_token,
        }
    }

    /// Set (or overwrite) the canonical URL (SEO §109).
    pub fn canonical(mut self, url: impl Into<String>) -> Self {
        self.canonical = url.into();
        self
    }

    /// Set (or overwrite) the meta description (SEO §109).
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Set the OpenGraph type ("website" | "article").
    pub fn og_type(mut self, og_type: &'static str) -> Self {
        self.og_type = og_type;
        self
    }

    /// Set (or overwrite) the CSRF token on an existing layout (for pages whose
    /// `current`/`title` are computed elsewhere, e.g. details).
    pub fn csrf(mut self, csrf: String) -> Self {
        self.csrf = csrf;
        self
    }
}

/// Error page (E1/E2), styled via Tailwind tokens.
#[derive(Template)]
#[template(path = "pages/error.html")]
pub struct ErrorPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub status: u16,
    pub message: String,
}

/// P1 — home / landing.
#[derive(Template)]
#[template(path = "pages/home.html")]
pub struct HomePage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub featured: Vec<view::CardVm>,
}

/// P2 — search results, full page.
#[derive(Template)]
#[template(path = "pages/search.html")]
pub struct SearchPageVm {
    pub layout: PageLayout,
    pub tr: Translator,
    pub results: view::ResultsData,
    pub form: http::SearchParams,
    pub security_options: Vec<view::OptionVm>,
    pub type_options: Vec<view::OptionVm>,
}

impl SearchPageVm {
    // Template-facing helpers: Askama's expression resolver can't compare
    // `Option<String>` directly, so selections are exposed as methods.
    fn sort_is(&self, code: &str) -> bool {
        self.form.sort == code
    }
    fn sort_none(&self) -> bool {
        self.form.sort.is_empty()
    }
    fn cost_is(&self, code: &str) -> bool {
        self.form.cost == code
    }
    fn cost_none(&self) -> bool {
        self.form.cost.is_empty()
    }
    fn open_now_checked(&self) -> bool {
        self.form.open_now == "true"
    }
    fn q_set(&self) -> bool {
        !self.form.q.is_empty()
    }
    fn sort_set(&self) -> bool {
        !self.form.sort.is_empty()
    }
    fn radius_is(&self, m: u32) -> bool {
        self.form.radius == Some(m)
    }
    fn radius_none(&self) -> bool {
        self.form.radius.is_none()
    }
}

/// HTMX fragment: only the results region.
#[derive(Template)]
#[template(path = "partials/search_results.html")]
pub struct SearchResultsVm {
    pub tr: Translator,
    pub results: view::ResultsData,
}

/// P3 — parking details.
#[derive(Template)]
#[template(path = "pages/parking_details.html")]
pub struct DetailsPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub id: i64,
    pub name: String,
    pub address: String,
    pub description: Option<String>,
    pub type_label: String,
    pub cost_label: String,
    pub rating_label: String,
    pub has_rating: bool,
    pub freshness_label: &'static str,
    pub freshness_code: &'static str,
    pub open_label: &'static str,
    pub open_code: &'static str,
    pub hours: Vec<view::HoursRowVm>,
    pub timezone_label: String,
    pub security: Vec<SecVm>,
    pub verified_label: String,
    pub osm_url: String,
    pub google_url: String,
    pub lat: f64,
    pub lon: f64,
    /// Approved location photos (presigned URLs), empty when none yet (P3
    /// gallery / M4 pipeline).
    pub gallery: Vec<PhotoVm>,
    // --- M3 community additions ---
    pub reviews: Vec<view::ReviewVm>,
    pub confidence_code: &'static str,
    pub confidence_label: String,
    pub disputed: bool,
    pub dispute_items: Vec<view::AttrDisputeVm>,
    pub parked_here_count: i64,
    pub is_favorited: bool,
    pub can_contribute: bool,
    pub is_authenticated: bool,
    pub has_own_review: bool,
    pub own_rating: u8,
    pub reasons: Vec<view::ReasonVm>,
    /// A one-time notice banner (post-action confirmation, e.g. "will be reviewed").
    pub notice: Option<String>,
    /// The location's §25 moderation state code (ACTIVE/PENDING_REVIEW/…). Public
    /// viewers only ever reach ACTIVE; moderators see a banner for the rest.
    pub moderation_state: &'static str,
    /// Whether the viewer is a moderator/admin (sees the hidden/invalid banner).
    pub is_moderator: bool,
    /// The report-reason options for the P3 report modal (§43).
    pub reason_options: Vec<view::OptionVm>,
}

/// One gallery photo: presigned URLs + accessible text. Grid tiles render the
/// (smaller) thumbnail; the lightbox renders the full derivative.
#[derive(Debug, Clone)]
pub struct PhotoVm {
    pub url: String,
    pub thumb_url: String,
    pub alt: String,
}

impl DetailsPage {
    pub fn build(tr: Translator, v: ParkingDetailsView, gallery: Vec<PhotoVm>, csrf: String) -> Self {
        use bikenest_domain::OpenStatus;
        let now = chrono::Utc::now();
        let loc = &v.location;
        let (lat, lon) = (loc.point().lat(), loc.point().lon());
        let open_code = match v.is_open_now {
            OpenStatus::Open => "open",
            OpenStatus::Closed => "closed",
            OpenStatus::Unknown => "unknown",
        };
        let open_label = view::open_label(tr, v.is_open_now);
        Self {
            layout: PageLayout::new(format!("{} — BikeNest", loc.name()), "")
                .csrf(csrf),
            tr,
            id: loc.id(),
            name: loc.name().to_string(),
            address: loc.address().to_string(),
            description: loc.description().map(str::to_string),
            type_label: view::type_label(tr, loc.parking_type()).to_string(),
            cost_label: view::cost_label(tr, loc.cost()),
            rating_label: view::rating_label(tr, loc.rating().avg(), loc.rating().count()),
            has_rating: loc.rating().avg().is_some(),
            freshness_label: view::freshness_label(tr, v.freshness),
            freshness_code: v.freshness.as_code(),
            open_label,
            open_code,
            hours: view::hours_rows(tr, loc.hours(), loc.timezone(), now),
            timezone_label: loc.timezone().name().to_string(),
            security: loc
                .security()
                .iter()
                .map(|f| SecVm {
                    label: tr.security(f.code()).to_string(),
                    state: match f.state() {
                        bikenest_domain::SecurityState::Yes => "yes",
                        bikenest_domain::SecurityState::No => "no",
                        bikenest_domain::SecurityState::Unknown => "unknown",
                    },
                })
                .collect(),
            verified_label: match loc.last_verified_at() {
                Some(t) => {
                    let days = (now - t).num_days();
                    if days == 0 {
                        tr.t("verified.today").to_string()
                    } else if days == 1 {
                        tr.t("verified.yesterday").to_string()
                    } else {
                        tr.t("verified.days_ago").replace("{n}", &days.to_string())
                    }
                }
                None => tr.t("verified.never").to_string(),
            },
            // §104: external navigation only — links to providers, coordinates
            // only (no user data is sent; the user leaves the app to navigate).
            osm_url: format!("https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=18/{lat}/{lon}"),
            google_url: format!("https://www.google.com/maps/dir/?api=1&destination={lat},{lon}"),
            lat,
            lon,
            gallery,
            reviews: Vec::new(),
            confidence_code: "reported",
            confidence_label: view::confidence_label(tr, bikenest_domain::Confidence::Reported)
                .to_string(),
            disputed: false,
            dispute_items: Vec::new(),
            parked_here_count: 0,
            is_favorited: false,
            can_contribute: false,
            is_authenticated: false,
            has_own_review: false,
            own_rating: 0,
            reasons: Vec::new(),
            notice: None,
            moderation_state: loc.moderation_state().as_code(),
            is_moderator: false,
            reason_options: view::report_reason_options(tr),
        }
    }

    /// Build the P3 page with the M3 community view (reviews, confidence,
    /// verification panel, favorite, recommendation explanation) overlaid on
    /// the base detail view. `viewer_verified` / `viewer_authenticated` gate the
    /// contributor actions; anonymous viewers get a public-only page.
    pub fn build_community(
        tr: Translator,
        v: bikenest_application::ParkingDetailsView,
        gallery: Vec<PhotoVm>,
        csrf: String,
        community: Option<bikenest_application::CommunityParkingDetails>,
        viewer_verified: bool,
        viewer_authenticated: bool,
        viewer_is_moderator: bool,
        storage: &dyn bikenest_application::ObjectStorage,
    ) -> Self {
        let mut page = Self::build(tr, v, gallery, csrf);
        let Some(c) = community else { return page };
        page.reviews = c.reviews.iter().map(|r| {
            let photos = c
                .review_photos
                .get(&r.id)
                .map(|ps| {
                    ps.iter()
                        .filter_map(|p| {
                            let url = view::resolve_photo(storage, Some(&p.key))?;
                            let thumb_url = p
                                .thumbnail_key
                                .as_deref()
                                .and_then(|k| view::resolve_photo(storage, Some(k)))
                                .unwrap_or_else(|| url.clone());
                            Some(PhotoVm {
                                url,
                                thumb_url,
                                alt: p
                                    .alt
                                    .clone()
                                    .unwrap_or_else(|| "Review photo".to_string()),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            view::review_vm(tr, r, false, photos)
        }).collect();
        page.confidence_code = c.confidence.as_code();
        page.confidence_label = view::confidence_label(tr, c.confidence).to_string();
        page.disputed = c.disputed;
        page.dispute_items = c
            .attribute_summary
            .iter()
            .filter(|a| a.incorrect > 0)
            .map(|a| view::attr_dispute_vm(tr, &a.code, a.incorrect))
            .collect();
        page.parked_here_count = c.parked_here_count;
        page.is_favorited = c.is_favorited;
        page.can_contribute = viewer_verified;
        page.is_authenticated = viewer_authenticated;
        page.has_own_review = c.own_review.is_some();
        page.own_rating = c.own_review.map(|r| r.rating.value()).unwrap_or(0);
        page.reasons = c.reasons.iter().map(|r| view::reason_vm(tr, r)).collect();
        page.is_moderator = viewer_is_moderator;
        page
    }

    /// Set (or overwrite) the page-level notice banner (e.g. "your change will
    /// be reviewed").
    pub fn notice(mut self, notice: Option<String>) -> Self {
        self.notice = notice;
        self
    }
}

pub struct SecVm {
    pub label: String,
    /// "yes" | "no" | "unknown"
    pub state: &'static str,
}

/// P7 — about / how it works.
#[derive(Template)]
#[template(path = "pages/about.html")]
pub struct AboutPage {
    pub layout: PageLayout,
    pub tr: Translator,
}

// ---------------------------------------------------------------------------
// Authentication & account pages (M2)
// ---------------------------------------------------------------------------

/// A1 — register.
#[derive(Template)]
#[template(path = "pages/register.html")]
pub struct RegisterPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub email: String,
    pub display_name: String,
    pub error: Option<String>,
}

/// A2 — login.
#[derive(Template)]
#[template(path = "pages/login.html")]
pub struct LoginPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub email: String,
    pub notice: Option<String>,
    pub error: Option<String>,
}

/// A3 — email verified (success or invalid/expired) + resend.
#[derive(Template)]
#[template(path = "pages/verify_email.html")]
pub struct VerifyEmailPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub success: bool,
    pub error: Option<String>,
}

/// A4 — request a password reset.
#[derive(Template)]
#[template(path = "pages/password_reset.html")]
pub struct PasswordResetPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub email: String,
    pub notice: Option<String>,
    pub error: Option<String>,
}

/// A5 — set a new password.
#[derive(Template)]
#[template(path = "pages/password_reset_new.html")]
pub struct PasswordResetNewPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub token: String,
    pub error: Option<String>,
}

/// C1 — account overview.
#[derive(Template)]
#[template(path = "pages/account.html")]
pub struct AccountPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub email: String,
    pub display_name: Option<String>,
    pub is_verified: bool,
    pub roles_label: String,
    pub notice: Option<String>,
}

/// C2 — change password.
#[derive(Template)]
#[template(path = "pages/account_password.html")]
pub struct AccountPasswordPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub error: Option<String>,
    pub notice: Option<String>,
}

/// C3 — change email.
#[derive(Template)]
#[template(path = "pages/account_email.html")]
pub struct AccountEmailPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub email: String,
    pub error: Option<String>,
    pub notice: Option<String>,
}

/// M5 — user management (role assignment).
#[derive(Template)]
#[template(path = "pages/admin_users.html")]
pub struct AdminUsersPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub users: Vec<view::AdminUserVm>,
    pub notice: Option<String>,
    pub error: Option<String>,
}

/// M6 — a versioned legal page (P4/P5/P6): current version + effective date.
#[derive(Template)]
#[template(path = "pages/policy.html")]
pub struct PolicyPage {
    pub layout: PageLayout,
    pub tr: Translator,
    /// Stable kind code ("privacy" | "terms" | "cookies").
    pub kind_code: &'static str,
    pub kind_label: &'static str,
    pub version: String,
    pub effective_label: String,
    /// Stored markdown, rendered escaped (never `|safe`).
    pub content: String,
}

/// M6 — the version history for a legal page (§70 determinability).
#[derive(Template)]
#[template(path = "pages/policy_versions.html")]
pub struct PolicyVersionsPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub kind_code: &'static str,
    pub kind_label: &'static str,
    pub items: Vec<view::PolicyVersionVm>,
}

/// M6 — C6 privacy & data hub.
#[derive(Template)]
#[template(path = "pages/account_privacy.html")]
pub struct AccountPrivacyPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub request_types: Vec<view::PrivacyRequestKindVm>,
    pub consent_records: bool,
    pub notice: Option<String>,
}

/// M6 — C7 export status.
#[derive(Template)]
#[template(path = "pages/account_export.html")]
pub struct AccountExportPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub items: Vec<view::ExportVm>,
    pub notice: Option<String>,
}

/// M6 — account deletion confirmation.
#[derive(Template)]
#[template(path = "pages/account_delete.html")]
pub struct AccountDeletePage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub error: Option<String>,
}

/// M6 — admin privacy-request queue.
#[derive(Template)]
#[template(path = "pages/admin_privacy_requests.html")]
pub struct AdminPrivacyRequestsPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub items: Vec<view::PrivacyRequestVm>,
    pub notice: Option<String>,
}

// ---------------------------------------------------------------------------
// M3 community pages
// ---------------------------------------------------------------------------

/// D1 — add a parking location.
#[derive(Template)]
#[template(path = "pages/parking_new.html")]
pub struct ParkingNewPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub name: String,
    pub address: String,
    pub description: String,
    pub parking_type: String,
    pub cost_kind: String,
    pub price: String,
    pub price_currency: String,
    pub price_unit: String,
    pub lat: String,
    pub lon: String,
    pub timezone: String,
    pub open_24h: bool,
    pub type_options: Vec<view::OptionVm>,
    pub security_options: Vec<view::OptionVm>,
    pub security: String,
    pub error: Option<String>,
    pub duplicates: Vec<view::DuplicateVm>,
    /// Set when the add succeeded but similar listings exist (advisory).
    pub added_id: Option<i64>,
}

/// D2 — edit a location (reversible fields).
#[derive(Template)]
#[template(path = "pages/parking_edit.html")]
pub struct ParkingEditPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub id: i64,
    pub version: i64,
    pub name: String,
    pub address: String,
    pub description: String,
    pub parking_type: String,
    pub cost_kind: String,
    pub price: String,
    pub price_currency: String,
    pub price_unit: String,
    pub open_24h: bool,
    pub type_options: Vec<view::OptionVm>,
    pub security_options: Vec<view::OptionVm>,
    pub security: String,
    pub error: Option<String>,
    pub notice: Option<String>,
}

/// D3 — write / edit a review.
#[derive(Template)]
#[template(path = "pages/review_form.html")]
pub struct ReviewFormPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub id: i64,
    pub rating: u8,
    pub body: String,
    pub error: Option<String>,
}

/// C4 — favorites list.
#[derive(Template)]
#[template(path = "pages/favorites.html")]
pub struct FavoritesPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub items: Vec<view::CardVm>,
    pub notice: Option<String>,
}

/// C5 — contribution history.
#[derive(Template)]
#[template(path = "pages/contributions.html")]
pub struct ContributionsPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub items: Vec<view::ContributionVm>,
}

/// HTMX fragment: the favorite button state.
#[derive(Template)]
#[template(path = "partials/favorite_button.html")]
pub struct FavoriteButtonVm {
    pub tr: Translator,
    pub id: i64,
    pub is_favorited: bool,
    pub csrf: String,
}

/// HTMX fragment: a short verification confirmation.
#[derive(Template)]
#[template(path = "partials/verification_result.html")]
pub struct VerificationResultVm {
    pub tr: Translator,
    pub label: String,
}

/// M2 photo moderation queue page (PLAN M4).
#[derive(Template)]
#[template(path = "pages/moderation_photos.html")]
pub struct ModerationPhotosPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub items: Vec<view::ModerationPhotoVm>,
    pub notice: Option<String>,
}

/// HTMX fragment: the P3 photo upload result (success or error).
#[derive(Template)]
#[template(path = "partials/photo_upload_result.html")]
pub struct PhotoUploadResultVm {
    pub tr: Translator,
    /// "success" | "error".
    pub state: &'static str,
    pub message: String,
}

// ---------------------------------------------------------------------------
// M5 moderation & reporting pages
// ---------------------------------------------------------------------------

/// M1 — moderation dashboard (counts + links to the queues).
#[derive(Template)]
#[template(path = "pages/moderation_dashboard.html")]
pub struct ModerationDashboardPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub pending_photos: usize,
    pub open_reports: usize,
    pub under_review_reports: usize,
    pub pending_proposals: usize,
    pub is_admin: bool,
}

/// M3 — reports queue.
#[derive(Template)]
#[template(path = "pages/moderation_reports.html")]
pub struct ModerationReportsPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub state_filter: String,
    pub items: Vec<view::ReportVm>,
    /// The current moderator's id — the template hides resolve/dismiss on one's
    /// own report (the server guard still enforces it).
    pub viewer_id: i64,
    pub notice: Option<String>,
}

/// M4 — proposal review queue.
#[derive(Template)]
#[template(path = "pages/moderation_proposals.html")]
pub struct ModerationProposalsPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub items: Vec<view::ProposalVm>,
    pub notice: Option<String>,
}

/// M6 — admin audit-log viewer.
#[derive(Template)]
#[template(path = "pages/admin_audit.html")]
pub struct AdminAuditPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub items: Vec<view::AuditRowVm>,
    pub next_cursor: Option<i64>,
    pub action: String,
    pub target_type: String,
    pub actor: String,
    pub from: String,
    pub to: String,
    pub notice: Option<String>,
}

/// Admin: a target user's contribution history (C5 aggregation scoped to a user).
#[derive(Template)]
#[template(path = "pages/admin_user_contributions.html")]
pub struct AdminUserContributionsPage {
    pub layout: PageLayout,
    pub tr: Translator,
    pub user_id: i64,
    pub email: String,
    pub items: Vec<view::ContributionVm>,
}

/// HTMX fragment: the report-submit result (success or error).
#[derive(Template)]
#[template(path = "partials/report_result.html")]
pub struct ReportResultVm {
    pub tr: Translator,
    pub state: &'static str,
    pub message: String,
}

/// HTMX fragment: a generic moderation-action toast.
#[derive(Template)]
#[template(path = "partials/moderation_action_result.html")]
pub struct ModerationActionResultVm {
    pub tr: Translator,
    pub state: &'static str,
    pub message: String,
}

pub fn app_router(db: bikenest_infrastructure::Db, probe_timeout: std::time::Duration) -> axum::Router {
    http::app_router(db, probe_timeout)
}

/// Test-oriented constructor: inject email/OAuth/password/rate-limiter providers
/// (tests pass a fast [`bikenest_test_support::TestPasswordHasher`] and a fresh
/// in-memory limiter to keep argon2 and ValKey out of the suite).
pub fn app_router_with<H: bikenest_application::PasswordHasher + Clone + 'static>(
    db: bikenest_infrastructure::Db,
    probe_timeout: std::time::Duration,
    email: Box<dyn bikenest_application::EmailProvider>,
    oauth: bikenest_infrastructure::FakeOAuthProvider,
    hasher: H,
    rate_limiter: Box<dyn bikenest_application::RateLimiter>,
) -> axum::Router {
    http::app_router_with(db, probe_timeout, email, oauth, hasher, rate_limiter)
}
