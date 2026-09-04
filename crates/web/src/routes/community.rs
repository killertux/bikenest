//! M3 contributions to a location: adding one, editing one, and proposing a
//! change to one — plus the form → domain mapping the three share.

use axum::extract::{Form, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use bikenest_application::{ContributionError, NewParkingLocation, ParkingEdit};
use bikenest_domain::{
    Cost, CurrencyCode, GeoPoint, ModerationState, Money, OpeningHours, ParkingLocation,
    ParkingType, PricingUnit, ProposalKind, ProposalPayload, ProposedChange, SecurityFeature,
    SecurityState, TimeRange, is_known_security_code,
};
use bikenest_infrastructure::MapConfig;

use crate::auth::Auth;
use crate::client_ip::ClientIp;
use crate::i18n::{Locale, Translator};
use crate::state::AppState;
use crate::view;
use crate::{PageLayout, ParkingEditPage, ParkingNewPage};

use super::common::render;
use super::errors::not_found_page;
use super::moderation::non_empty;

pub(crate) fn parse_bool(s: &str) -> bool {
    s == "true" || s == "1" || s == "on"
}

pub(crate) fn security_from_form(s: &str) -> Vec<SecurityFeature> {
    s.split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .filter(|c| is_known_security_code(c))
        .map(|c| SecurityFeature::new(c, SecurityState::Yes))
        .collect()
}

/// Parse a price in major units ("5", "5.50", "5,50") into cents.
pub(crate) fn parse_price_major_to_cents(raw: &str) -> Option<i64> {
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
pub(crate) fn cents_to_major_string(cents: i64) -> String {
    let major = cents / 100;
    let frac = (cents % 100).abs();
    if frac == 0 {
        major.to_string()
    } else {
        format!("{major}.{frac:02}")
    }
}

pub(crate) fn cost_from_form(form: &NewParkingForm) -> Result<Cost, ContributionError> {
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

pub(crate) fn new_location_from_form(
    form: &NewParkingForm,
) -> Result<NewParkingLocation, ContributionError> {
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

pub(crate) fn edit_from_form(
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
pub(crate) fn cost_kind_string(cost: &Cost) -> String {
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
pub(crate) fn cost_price_strings(cost: &Cost) -> (String, String, String) {
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
pub(crate) fn hours_open_24h(hours: &OpeningHours) -> bool {
    matches!(hours, OpeningHours::Weekly(rows) if !rows.is_empty() && rows.iter().all(|(_, r)| r.all_day))
}

/// Comma-separated codes of the security attributes confirmed `yes` (to
/// pre-fill the add/edit checkboxes).
pub(crate) fn security_yes_codes_string(loc: &ParkingLocation) -> String {
    loc.security()
        .iter()
        .filter(|f| f.state() == SecurityState::Yes)
        .map(|f| f.code())
        .collect::<Vec<_>>()
        .join(",")
}

/// Build a `ParkingEditPage` with all reversible fields pre-filled from `loc`.
/// The page chrome shared by every render of the "edit parking" form.
pub(crate) fn edit_parking_layout(map: &MapConfig, tr: Translator, auth: &Auth) -> PageLayout {
    PageLayout::for_request(tr.t("edit.title").to_string(), "edit", auth, map)
}

/// The page chrome shared by every render of the "add parking" form.
pub(crate) fn new_parking_layout(map: &MapConfig, tr: Translator, auth: &Auth) -> PageLayout {
    PageLayout::for_request(tr.t("new.title").to_string(), "new", auth, map)
}

pub(crate) fn parking_edit_page_vm(
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
pub(crate) struct NewParkingForm {
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

pub(crate) async fn parking_new_page(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
) -> Response {
    let tr = Translator::new(locale);
    if let Err(resp) = auth.require_verified() {
        return resp;
    }
    render(
        ParkingNewPage {
            layout: PageLayout::for_request(
                tr.t("new.title").to_string(),
                "new",
                &auth,
                &state.map,
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
pub(crate) async fn parking_new_post(
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
                // Advisory: the location was added, but similar listings
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

pub(crate) fn render_new_form(
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

pub(crate) fn render_form_error(
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

pub(crate) fn contribution_error_message(tr: Translator, e: &ContributionError) -> String {
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
pub(crate) fn contribution_error_status(e: &ContributionError, default: StatusCode) -> StatusCode {
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
pub(crate) struct EditParkingForm {
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

pub(crate) async fn parking_edit_page(
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
        return not_found_page(&headers, &state.map, &auth, tr);
    };
    // A location moderation took down accepts no contributions, so the form
    // (and the sensitive-change proposals it hosts) is gone for everyone —
    // moderators included, since the write path refuses them too.
    if view.location.moderation_state() != ModerationState::Active {
        return not_found_page(&headers, &state.map, &auth, tr);
    }
    let loc = &view.location;
    // Pre-fill every reversible field so editing one doesn't silently reset
    // cost/security/hours (the editable fields arrive pre-filled).
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

pub(crate) async fn parking_edit_post(
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
        _ => return not_found_page(&headers, &state.map, &auth, tr),
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
            // A concurrent edit won: reload the latest values and tell the user.
            let Some(view) = state.details.execute(id).await.ok().flatten() else {
                return not_found_page(&headers, &state.map, &auth, tr);
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
        Err(ContributionError::LocationNotActive) => {
            not_found_page(&headers, &state.map, &auth, tr)
        }
        Err(e) => contribution_edit_error(&state.map, tr, auth, id, &form, &e),
    }
}

pub(crate) fn contribution_edit_error(
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

pub(crate) fn contribution_edit_notice(
    map: &MapConfig,
    tr: Translator,
    auth: Auth,
    id: i64,
    form: &EditParkingForm,
    notice: String,
) -> Response {
    contribution_edit_notice_status(map, tr, auth, id, form, notice, StatusCode::OK)
}

pub(crate) fn contribution_edit_notice_status(
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
            layout: PageLayout::for_request(tr.t("edit.title").to_string(), "edit", &auth, map),
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
pub(crate) struct ProposalForm {
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

pub(crate) async fn parking_proposal_post(
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
    // Build the typed payload and let it render its own JSON, so the stored
    // shape is defined in one place (the domain) instead of by a `json!` here
    // and a hand-written reader in the moderation queue.
    let change = match kind {
        ProposalKind::MoveLocation => {
            let Ok(lat) = form.lat.trim().parse::<f64>() else {
                return proposal_error(id);
            };
            let Ok(lon) = form.lon.trim().parse::<f64>() else {
                return proposal_error(id);
            };
            ProposedChange::MoveLocation {
                lat,
                lon,
                timezone: non_empty(&form.timezone),
            }
        }
        ProposalKind::ChangeExistence => match parse_proposed_existence(&form.existence) {
            Some(exists) => ProposedChange::ChangeExistence { exists },
            None => return proposal_error(id),
        },
    };
    // An out-of-range coordinate would round-trip as `Unknown`; refuse it at
    // the door instead of filing a proposal no moderator can act on.
    if change == ProposedChange::Unknown {
        return proposal_error(id);
    }
    let proposed = ProposalPayload::new(change, Some(&form.reason)).to_json();
    match state
        .contributions
        .propose_location_change(user, id, kind, proposed)
        .await
    {
        Ok(_) => axum::response::Redirect::to(&format!("/parking/{id}?proposed=1")).into_response(),
        // The proposal forms live on the edit page, which 404s for a
        // taken-down location; a direct POST gets the same answer rather than
        // a redirect to a page that would itself 404.
        Err(ContributionError::LocationNotActive) => {
            not_found_page(&headers, &state.map, &auth, tr)
        }
        Err(_) => proposal_error(id),
    }
}

/// Read the existence radio on the "propose removal" form.
///
/// Two vocabularies reach this field. `removed`/`exists` are the payload's own
/// codes (what the moderation queue's approve form posts). `no_longer_exists`
/// and `info_changed` are the *verification* codes the edit page's radios have
/// posted since M3 — they were stored verbatim and the queue, which only knew
/// `removed`, read `no_longer_exists` as "still exists", quietly inverting
/// every removal proposal a rider filed. Both vocabularies are mapped here, so
/// the stored payload is canonical whichever form submitted it.
pub(crate) fn parse_proposed_existence(raw: &str) -> Option<bool> {
    match raw.trim() {
        "removed" | "no_longer_exists" => Some(false),
        // "the information changed" is not a removal: the spot is still there.
        "exists" | "info_changed" | "still_exists" => Some(true),
        _ => None,
    }
}

/// The proposal forms live on the details/edit page; a rejected submission
/// returns there with an error flag rather than rendering a bare 400.
pub(crate) fn proposal_error(id: i64) -> Response {
    axum::response::Redirect::to(&format!("/parking/{id}?proposal_error=1")).into_response()
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
}
