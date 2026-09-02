//! BikeNest web crate: axum routing, handlers, Askama templates.

pub mod auth;
pub mod http;
pub mod i18n;
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
}

impl PageLayout {
    /// A public page layout (no CSRF token).
    pub fn new(title: String, current: &str) -> Self {
        Self {
            title,
            current: current.to_string(),
            csrf: String::new(),
        }
    }

    /// A layout carrying the session's CSRF token (for authenticated forms).
    pub fn with_csrf(title: String, current: &str, csrf: String) -> Self {
        Self {
            title,
            current: current.to_string(),
            csrf,
        }
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
}

/// One gallery photo: presigned URLs + accessible text. Grid tiles render the
/// (smaller) thumbnail; the lightbox renders the full derivative.
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
    ) -> Self {
        let mut page = Self::build(tr, v, gallery, csrf);
        let Some(c) = community else { return page };
        page.reviews = c.reviews.iter().map(|r| view::review_vm(tr, r, false)).collect();
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

pub fn app_router(db: bikenest_infrastructure::Db, probe_timeout: std::time::Duration) -> axum::Router {
    http::app_router(db, probe_timeout)
}

/// Test-oriented constructor: inject email/OAuth/password providers (tests pass
/// a fast [`bikenest_test_support::TestPasswordHasher`] to keep argon2 out of
/// the suite).
pub fn app_router_with(
    db: bikenest_infrastructure::Db,
    probe_timeout: std::time::Duration,
    email: Box<dyn bikenest_application::EmailProvider>,
    oauth: bikenest_infrastructure::FakeOAuthProvider,
    hasher: Box<dyn bikenest_application::PasswordHasher>,
) -> axum::Router {
    http::app_router_with(db, probe_timeout, email, oauth, hasher)
}
