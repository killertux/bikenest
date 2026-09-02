//! BikeNest web crate: axum routing, handlers, Askama templates.

pub mod http;
pub mod i18n;
pub mod view;

use askama::Template;
use bikenest_application::ParkingDetailsView;
use i18n::Translator;

/// Base layout data shared by all pages. `current` drives the active nav item.
pub struct PageLayout {
    pub title: String,
    pub current: String,
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
}

/// One gallery photo: a ready-to-render (presigned) URL + accessible text.
pub struct PhotoVm {
    pub url: String,
    pub alt: String,
}

impl DetailsPage {
    pub fn build(tr: Translator, v: ParkingDetailsView, gallery: Vec<PhotoVm>) -> Self {
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
            layout: PageLayout {
                title: format!("{} — BikeNest", loc.name()),
                current: String::new(),
            },
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
        }
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

pub fn app_router(db: bikenest_infrastructure::Db, probe_timeout: std::time::Duration) -> axum::Router {
    http::app_router(db, probe_timeout)
}
