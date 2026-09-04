//! Pages that are always there: the landing page, `/about`, the crawler
//! files (`robots.txt`, `sitemap.xml`), the language switch and the
//! health/readiness probes.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use bikenest_application::{Readiness, SearchInput};
use bikenest_infrastructure::FEATURED_ORIGIN;
use serde_json::json;

use crate::auth::Auth;
use crate::htmx;
use crate::i18n::{Locale, Translator};
use crate::state::AppState;
use crate::view::{self, CardVm};
use crate::{AboutPage, HomePage, PageLayout};

use super::common::{locale_code, render};

pub(crate) async fn healthz() -> &'static str {
    "ok"
}

pub(crate) async fn readyz(State(state): State<AppState>) -> Response {
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
            Json(json!({"status": "error"})), // no internal details
        )
            .into_response(),
    }
}

/// GET /robots.txt — the indexing policy. Public pages are crawlable;
/// private account/admin/moderation paths are disallowed here (and also get
/// `X-Robots-Tag: noindex` on their responses).
pub(crate) async fn robots_txt() -> Response {
    const BODY: &str =
        "User-agent: *\nAllow: /\nDisallow: /account\nDisallow: /admin\nDisallow: /moderation\n";
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], BODY).into_response()
}

/// GET /sitemap.xml — the static pages plus every ACTIVE public parking id.
pub(crate) async fn sitemap_xml(State(state): State<AppState>) -> Response {
    let base = state.base_url.clone();
    let static_urls = ["/", "/search", "/about", "/privacy", "/terms", "/cookies"];
    let parking_ids: Vec<i64> = state.sitemap.active_parking_ids().await.unwrap_or_default();

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

/// P1 — home / landing.
pub(crate) async fn home(State(state): State<AppState>, locale: Locale, auth: Auth) -> Response {
    let tr = Translator::new(locale);
    // A few example locations near the featured landmark, when data exists
    // (P1: optional section). Failure → render without them.
    let mut featured = Vec::new();
    if let Ok((page, _)) = state
        .search
        .execute(SearchInput {
            // Coordinates, not the landmark's name: the strip is a constant
            // destination, and geocoding it on every home render was a
            // billable provider call per view. With `lat`/`lon` set the use
            // case never reaches the geocoder at all, and the
            // recommended sort now honours `page_size` in SQL, so this asks
            // the database for five rows rather than five hundred.
            lat: Some(FEATURED_ORIGIN.0),
            lon: Some(FEATURED_ORIGIN.1),
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
        layout: PageLayout::for_request(tr.t("home.title").to_string(), "home", &auth, &state.map)
            .canonical(format!("{}/", state.base_url.trim_end_matches('/')))
            .description(tr.t("home.hero.subtitle").to_string()),
        tr,
        featured,
    };
    render(page, StatusCode::OK)
}

/// P7 — about / how it works.
pub(crate) async fn about(State(state): State<AppState>, locale: Locale, auth: Auth) -> Response {
    let tr = Translator::new(locale);
    let page = AboutPage {
        layout: PageLayout::for_request(
            tr.t("about.title").to_string(),
            "about",
            &auth,
            &state.map,
        )
        .canonical(format!("{}/about", state.base_url.trim_end_matches('/')))
        .description(tr.t("about.title").to_string()),
        tr,
    };
    render(page, StatusCode::OK)
}

/// Language toggle: set the `lang` cookie and return to `next` (a local
/// path only) or the referring page. Unknown codes just redirect home.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct LangParams {
    #[serde(default)]
    next: String,
}

pub(crate) async fn set_lang(
    State(state): State<AppState>,
    Path(code): Path<String>,
    Query(params): Query<LangParams>,
    auth: Auth,
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
    // Persist the choice for a signed-in user. The cookie only changes what
    // this browser renders; `users.locale` is what the background job that
    // sends their next verification or password-reset mail reads. A write
    // failure is not worth failing the redirect over — the interface has
    // already switched, and the language toggle is not a transaction.
    if let Some(user) = auth.user.as_ref()
        && let Err(err) = state.auth.set_locale(user.id, locale_code(locale)).await
    {
        tracing::warn!(error = %err, "could not persist the account language");
    }
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
