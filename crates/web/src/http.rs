//! HTTP routing and handlers.

use askama::Template;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::get;
use axum::{Router, extract::Path, extract::Query, extract::State};
use bikenest_application::{
    CheckReadiness, GetParkingDetails, ObjectStorage, ParkingPhotoReader, Readiness, SearchInput,
    SearchParking,
};
use bikenest_infrastructure::probe::SqlxDatabaseProbe;
use bikenest_infrastructure::{
    Db, FakeGeocoder, LocalDiskStorage, SqlxParkingDetailsReader, SqlxParkingPhotoReader,
    SqlxParkingSearchReader,
};
use serde_json::json;
use std::sync::Arc;

use crate::i18n::{Locale, Translator};
use crate::view::{self, CardVm, ResultsData};
use crate::{DetailsPage, ErrorPage, HomePage, PageLayout, PhotoVm, SearchPageVm, AboutPage, SearchResultsVm};

/// Shared application state wired at startup.
#[derive(Clone)]
pub struct AppState {
    pub readiness: Arc<CheckReadiness<SqlxDatabaseProbe>>,
    pub search: Arc<SearchParking>,
    pub details: Arc<GetParkingDetails>,
    pub photos: Arc<dyn ParkingPhotoReader>,
    pub storage: Arc<dyn ObjectStorage>,
}

/// Builds the full application router with a real database handle.
pub fn app_router(db: Db, probe_timeout: std::time::Duration) -> Router {
    let probe = SqlxDatabaseProbe::new(db.clone(), probe_timeout);
    let search_uc = SearchParking::new(
        Box::new(FakeGeocoder),                                   // Ledger #2
        Box::new(SqlxParkingSearchReader::new(db.clone())),
        bikenest_application::DEFAULT_RECOMMENDATION_CONFIG,
        Default::default(),
    );
    let details = GetParkingDetails::new(
        Box::new(SqlxParkingDetailsReader::new(db.clone())),
        Default::default(),
    );
    let state = AppState {
        readiness: Arc::new(CheckReadiness::new(probe)),
        search: Arc::new(search_uc),
        details: Arc::new(details),
        photos: Arc::new(SqlxParkingPhotoReader::new(db.clone())),
        storage: Arc::new(LocalDiskStorage::from_env()), // Ledger #7
    };
    Router::new()
        .route("/", get(home))
        .route("/search", get(search))
        .route("/parking/{id}", get(parking_details))
        .route("/about", get(about))
        .route("/lang/{code}", get(set_lang))
        .route("/media/{*key}", get(media))
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .nest_service(
            "/static",
            tower_http::services::ServeDir::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../web/static"
            )),
        )
        .fallback(not_found)
        .with_state(state)
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz(State(state): State<AppState>) -> Response {
    match state.readiness.execute().await {
        Readiness::Ready => (StatusCode::OK, Json(json!({"status": "ready", "database": "up"}))).into_response(),
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

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

/// P1 — home / landing.
async fn home(State(state): State<AppState>, locale: Locale) -> Response {
    let tr = Translator::new(locale);
    // A few example locations near the featured landmark, when data exists
    // (UI_DESIGN P1: optional section). Failure → render without them.
    let featured = state
        .search
        .execute(SearchInput {
            query: Some("Rua XV de Novembro".to_string()),
            radius_m: Some(1000),
            page_size: Some(4),
            ..Default::default()
        })
        .await
        .map(|(page, _)| {
            let now = chrono::Utc::now();
            page.items
                .iter()
                .map(|s| {
                    let photo_url =
                        view::resolve_photo(&*state.storage, s.photo_key.as_deref());
                    CardVm::from_summary(
                        tr,
                        s,
                        bikenest_domain::categorize(
                            s.last_verified_at,
                            now,
                            &bikenest_domain::DEFAULT_THRESHOLDS,
                        ),
                        photo_url,
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let page = HomePage {
        layout: PageLayout {
            title: tr.t("home.title").to_string(),
            current: "home".to_string(),
        },
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
    params: Query<SearchParams>,
) -> Response {
    let tr = Translator::new(locale);
    let is_htmx = headers.contains_key("hx-request");
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
                &*state.storage,
            )
        }
        Err(bikenest_application::SearchError::MissingDestination) => ResultsData {
            destination_label: None,
            total_label: String::new(),
            items: Vec::new(),
            cursor_url: None,
            error: Some(tr.t("search.missing").to_string()),
            map_json: serde_json::json!({ "origin": null, "items": [] }).to_string(),
        },
        Err(_) => return internal_error(tr),
    };

    if is_htmx {
        let vm = SearchResultsVm { tr, results };
        render(vm, StatusCode::OK)
    } else {
        let vm = SearchPageVm {
            layout: PageLayout {
                title: tr.t("search.title").to_string(),
                current: "search".to_string(),
            },
            tr,
            results,
            form: params.0.clone(),
            security_options: view::security_options(tr, Some(&params.security)),
            type_options: view::type_options(tr, Some(&params.parking_type)),
        };
        render(vm, StatusCode::OK)
    }
}

/// P3 — parking details.
async fn parking_details(
    State(state): State<AppState>,
    locale: Locale,
    Path(id): Path<i64>,
) -> Response {
    let tr = Translator::new(locale);
    match state.details.execute(id).await {
        Ok(Some(view)) => {
            // Approved photos (P3 gallery). A read failure degrades to no
            // gallery rather than failing the page.
            let gallery = match state.photos.photos(id).await {
                Ok(photos) => {
                    let name = view.location.name().to_string();
                    photos
                        .into_iter()
                        .filter_map(|p| {
                            view::resolve_photo(&*state.storage, Some(&p.key)).map(|url| PhotoVm {
                                url,
                                alt: p.alt.unwrap_or_else(|| format!("Photo of {name}")),
                            })
                        })
                        .collect()
                }
                Err(_) => Vec::new(),
            };
            let page = DetailsPage::build(tr, view, gallery);
            render(page, StatusCode::OK)
        }
        Ok(None) => not_found_page(tr),
        Err(_) => internal_error(tr),
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

/// P7 — about / how it works.
async fn about(locale: Locale) -> Response {
    let tr = Translator::new(locale);
    let page = AboutPage {
        layout: PageLayout {
            title: tr.t("about.title").to_string(),
            current: "about".to_string(),
        },
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
    // reports (boosted request), else the Referer — all reduced to a local,
    // single-slash path (open-redirect guard).
    let from_header = |name: &str| -> Option<String> {
        let raw = headers.get(name)?.to_str().ok()?;
        // Strip scheme + host to keep only the local path (+ query).
        let path = raw.find("://").map(|i| {
            raw[i + 3..].find('/').map(|j| &raw[i + 3 + j..]).unwrap_or("/")
        });
        let path = path.unwrap_or(raw);
        (path.starts_with('/') && !path.starts_with("//")).then(|| path.to_string())
    };
    let next = if params.next.starts_with('/') && !params.next.starts_with("//") {
        params.next.clone()
    } else {
        from_header("hx-current-url")
            .or_else(|| from_header("referer"))
            .unwrap_or_else(|| "/".to_string())
    };
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

fn render<T: Template>(template: T, status: StatusCode) -> Response {
    match template.render() {
        Ok(html) => (status, Html(html)).into_response(),
        // A render failure is a bug; keep the fallback minimal (no template).
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response(),
    }
}

fn error_page(tr: Translator, status: StatusCode, title_key: &str, body_key: &str) -> Response {
    let page = ErrorPage {
        layout: PageLayout {
            title: format!("{} — BikeNest", tr.t(title_key)),
            current: String::new(),
        },
        tr,
        status: status.as_u16(),
        message: tr.t(body_key).to_string(),
    };
    match page.render() {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => (status, tr.t(body_key)).into_response(),
    }
}

fn internal_error(tr: Translator) -> Response {
    error_page(tr, StatusCode::INTERNAL_SERVER_ERROR, "error.500.title", "error.500.body")
}

fn not_found_page(tr: Translator) -> Response {
    error_page(tr, StatusCode::NOT_FOUND, "error.404.title", "error.404.body")
}

/// Router fallback (E1). Resolves locale from the request for a translated 404.
async fn not_found(locale: Locale) -> Response {
    not_found_page(Translator::new(locale))
}
