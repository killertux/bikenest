//! Small JSON endpoints the progressive-enhancement layer calls.
//!
//! Only one so far: `GET /api/geocode`, which the add/edit map picker uses to
//! turn the address the contributor already typed into a starting position for
//! the pin. It exists so a phone contributor never has to know a decimal
//! latitude — but it reaches the same billable provider `/search` does, so it
//! carries the same two guards: signed-in *and* verified (the contribution
//! gate), and the per-IP geocode budget. A destination the in-process cache
//! can already answer costs the provider nothing and is therefore free.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use bikesnest_application::Geocoder;

use crate::auth::Auth;
use crate::client_ip::ClientIp;
use crate::state::AppState;

use super::search::geocode_within_budget;

#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct GeocodeQuery {
    #[serde(default)]
    q: String,
}

/// `GET /api/geocode?q=…` → `{"lat":…,"lon":…,"label":"…"}`.
///
/// `404 {}` when nothing matches, `429 {}` when this network has spent its
/// geocode budget, `503 {}` when the provider is unreachable. The picker
/// treats every non-200 the same way: keep the pin where it is and say so.
pub(crate) async fn geocode_api(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    auth: Auth,
    Query(q): Query<GeocodeQuery>,
) -> Response {
    if let Err(resp) = auth.require_verified() {
        return resp;
    }
    let query = q.q.trim();
    if query.is_empty() {
        return json(StatusCode::NOT_FOUND, "{}".to_string());
    }

    // A cached answer is free, so it must not be charged — same rule as the
    // search page's budget check.
    let cached = state.geocoder.peek(query);
    if cached.is_none() && !geocode_within_budget(&state, &ip).await {
        return json(StatusCode::TOO_MANY_REQUESTS, "{}".to_string());
    }

    let hit = match cached {
        Some(hit) => Some(hit),
        None => match state.geocoder.geocode(query).await {
            Ok(hit) => hit,
            Err(_) => return json(StatusCode::SERVICE_UNAVAILABLE, "{}".to_string()),
        },
    };
    match hit {
        Some(hit) => json(
            StatusCode::OK,
            serde_json::json!({
                "lat": hit.point.lat(),
                "lon": hit.point.lon(),
                "label": hit.label,
            })
            .to_string(),
        ),
        None => json(StatusCode::NOT_FOUND, "{}".to_string()),
    }
}

fn json(status: StatusCode, body: String) -> Response {
    // Answers are per-user and per-budget; a shared cache must never replay
    // one caller's result (or 429) to another.
    (
        status,
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/json; charset=utf-8",
            ),
            (axum::http::header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
}
