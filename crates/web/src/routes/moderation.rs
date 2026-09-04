//! M5 reports, the moderation queues (reports, proposals) and every
//! moderator action on a review, a proposal or a location.

use axum::extract::{Form, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use bikenest_application::{ModerationError, ProposalField, ProposalOverride};
use bikenest_domain::{ReportOutcome, ReportState, ReportTargetType, Role};
use bikenest_infrastructure::MapConfig;

use crate::auth::Auth;
use crate::client_ip::ClientIp;
use crate::htmx;
use crate::i18n::{Locale, Translator};
use crate::state::AppState;
use crate::view;
use crate::{
    ModerationActionResultVm, ModerationDashboardPage, ModerationProposalsPage,
    ModerationReportsPage, PageLayout, ReportResultVm,
};

use super::common::{DEFAULT_PAGE_LIMIT, fragment_answer, parse_after_id, render};

/// The `?done=…` flag a moderation action redirects a whole-document request
/// back to its queue with, plus the queue's keyset cursor. The `done` value
/// names the action, not the message key, so the URL never carries catalog
/// internals.
#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct ModerationNotice {
    #[serde(default)]
    done: String,
    /// Keyset cursor (photo queue only: `created_at` of the last item on the
    /// previous page, RFC3339). Empty = first page.
    #[serde(default)]
    pub(crate) after_at: String,
    /// Keyset cursor (the last item's id on the previous page). `0` = first page.
    #[serde(default)]
    pub(crate) after_id: i64,
}

/// The queue banner for a `?done=…` flag, or nothing for an unknown value.
pub(crate) fn moderation_notice(tr: Translator, q: &ModerationNotice) -> Option<String> {
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

/// A tiny rejection form: the moderator's reason. Shared by the photo queue
/// and the proposal queue.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct RejectReasonForm {
    #[serde(default)]
    pub(crate) reason: String,
}

/// Render a swap-safe moderation-action toast (or, for a whole-document
/// request, redirect back to the queue / show the styled error page).
pub(crate) fn moderation_result(
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
pub(crate) fn moderation_error_message(
    tr: Translator,
    e: &ModerationError,
) -> (StatusCode, String) {
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
        InvalidProposalField(field) => (
            StatusCode::BAD_REQUEST,
            match field {
                ProposalField::Lat => "proposal.error.lat",
                ProposalField::Lon => "proposal.error.lon",
                ProposalField::Timezone => "proposal.error.timezone",
                ProposalField::Existence => "proposal.error.existence",
            },
        ),
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

pub(crate) fn report_result(
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
pub(crate) struct ReportForm {
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
pub(crate) fn report_return_url(form: &ReportForm) -> String {
    if let Some(local) = htmx::safe_local_path(&form.page) {
        let sep = if local.contains('?') { '&' } else { '?' };
        return format!("{local}{sep}reported=1");
    }
    if form.target_type == "parking" && form.target_id > 0 {
        return format!("/parking/{}?reported=1", form.target_id);
    }
    "/".to_string()
}

pub(crate) async fn report_submit(
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
            tracing::info!("report submitted"); // no PII in the log field
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
pub(crate) async fn moderation_dashboard(
    State(state): State<AppState>,
    locale: Locale,
    auth: Auth,
) -> Response {
    let tr = Translator::new(locale);
    let user = match auth.require_moderator() {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    // One statement (four scalar subqueries) instead of loading and
    // `.len()`-ing four full lists.
    let counts = state
        .moderation
        .queue_counts(user)
        .await
        .unwrap_or_default();
    let is_admin = user.has_role(Role::Admin);
    render(
        ModerationDashboardPage {
            layout: PageLayout::for_request(
                tr.t("moderation.dashboard.title").to_string(),
                "moderation",
                &auth,
                &state.map,
            ),
            tr,
            pending_photos: counts.pending_photos,
            open_reports: counts.open_reports,
            under_review_reports: counts.under_review_reports,
            pending_proposals: counts.pending_proposals,
            is_admin,
        },
        StatusCode::OK,
    )
}

/// GET /moderation/reports — the M3 reports queue (optional `?state=` filter).
#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct ReportFilterQuery {
    #[serde(default)]
    state: String,
    /// Set by a moderation action redirecting a whole-document request back here.
    #[serde(default)]
    done: String,
    /// Keyset cursor: the last report id from the previous page. `0` = first page.
    #[serde(default)]
    pub(crate) after_id: i64,
}

pub(crate) async fn moderation_reports(
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
    let reports = state
        .moderation
        .list_reports(
            user,
            state_filter,
            parse_after_id(q.after_id),
            DEFAULT_PAGE_LIMIT,
        )
        .await
        .unwrap_or_default();
    let next_url = (reports.len() as i64 == DEFAULT_PAGE_LIMIT)
        .then(|| reports.last())
        .flatten()
        .map(|last| format!("/moderation/reports?state={}&after_id={}", q.state, last.id));
    // One batched lookup for the whole page: what each report actually points
    // at. Without it the queue can only show `#4057`.
    let previews = state
        .moderation
        .report_previews(user, &reports)
        .await
        .unwrap_or_default();
    let mut items = Vec::with_capacity(reports.len());
    for r in &reports {
        let preview = previews.get(&(r.target_type, r.target_id));
        let thumb = match preview {
            Some(p) => {
                let key = p.photo_thumbnail_key.as_deref().or(p.photo_key.as_deref());
                view::resolve_photo(&*state.storage, key).await
            }
            None => None,
        };
        items.push(view::report_vm(tr, r, preview, thumb));
    }
    render(
        ModerationReportsPage {
            layout: PageLayout::for_request(
                tr.t("moderation.reports.title").to_string(),
                "moderation",
                &auth,
                &state.map,
            ),
            tr,
            state_filter: q.state,
            items,
            viewer_id: user.id.0,
            notice: moderation_notice(
                tr,
                &ModerationNotice {
                    done: q.done,
                    ..Default::default()
                },
            ),
            next_url,
        },
        StatusCode::OK,
    )
}

/// The reports queue a moderation action returns a whole-document request to.
pub(crate) fn reports_queue_url(done: &str) -> String {
    format!("/moderation/reports?done={done}")
}

/// POST /moderation/reports/{id}/claim — claim an open report.
pub(crate) async fn moderation_report_claim(
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
        Ok(()) => (
            "success",
            tr.t("report.claimed").to_string(),
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
        &reports_queue_url("claimed"),
    )
}

#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct ResolutionForm {
    #[serde(default)]
    note: String,
}

/// POST /moderation/reports/{id}/resolve — resolve a claimed report (HTMX).
pub(crate) async fn moderation_report_resolve(
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
            tracing::info!("report resolved"); // no PII in the log field
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
pub(crate) async fn moderation_report_dismiss(
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
pub(crate) async fn moderation_proposals(
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
    let proposals = state
        .moderation
        .list_pending_proposals(user, parse_after_id(q.after_id), DEFAULT_PAGE_LIMIT)
        .await
        .unwrap_or_default();
    let next_url = (proposals.len() as i64 == DEFAULT_PAGE_LIMIT)
        .then(|| proposals.last())
        .flatten()
        .map(|last| format!("/moderation/proposals?after_id={}", last.id));
    let items = proposals
        .into_iter()
        .map(|p| view::proposal_vm(tr, &p))
        .collect();
    render(
        ModerationProposalsPage {
            layout: PageLayout::for_request(
                tr.t("moderation.proposals.title").to_string(),
                "moderation",
                &auth,
                &state.map,
            ),
            tr,
            items,
            notice: moderation_notice(tr, &q),
            next_url,
        },
        StatusCode::OK,
    )
}

/// The proposals queue a moderation action returns a whole-document request to.
pub(crate) fn proposals_queue_url(done: &str) -> String {
    format!("/moderation/proposals?done={done}")
}

#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct ApproveProposalForm {
    #[serde(default)]
    lat: String,
    #[serde(default)]
    lon: String,
    #[serde(default)]
    timezone: String,
    #[serde(default)]
    existence: String,
}

impl ApproveProposalForm {
    /// Map the form to a [`ProposalOverride`]. This is the handler's whole
    /// contribution to an approval: an empty input is simply an absent
    /// override, and a typo is an error rather than silently reading as "keep
    /// the proposer's value". The merge rule itself lives in the application
    /// layer, where it is testable, instead of being re-derived from string
    /// emptiness here (A-M6).
    fn to_override(&self) -> Result<ProposalOverride, ModerationError> {
        Ok(ProposalOverride {
            lat: parse_optional_f64(&self.lat, ProposalField::Lat)?,
            lon: parse_optional_f64(&self.lon, ProposalField::Lon)?,
            timezone: non_empty(&self.timezone),
            exists: match self.existence.trim() {
                "exists" => Some(true),
                "removed" => Some(false),
                _ => None,
            },
        })
    }
}

pub(crate) fn parse_optional_f64(
    raw: &str,
    field: ProposalField,
) -> Result<Option<f64>, ModerationError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    raw.parse::<f64>()
        .map(Some)
        .map_err(|_| ModerationError::InvalidProposalField(field))
}

pub(crate) fn non_empty(raw: &str) -> Option<String> {
    let raw = raw.trim();
    (!raw.is_empty()).then(|| raw.to_string())
}

/// POST /moderation/proposals/{id}/approve — approve a proposal (optionally adjusted).
pub(crate) async fn moderation_proposal_approve(
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
    let over = match form.to_override() {
        Ok(o) => o,
        Err(e) => return fail(&e),
    };
    match state.moderation.approve_proposal(user, id, over).await {
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
pub(crate) async fn moderation_proposal_reject(
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
pub(crate) async fn moderation_review_hide(
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
pub(crate) async fn moderation_review_restore(
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
        Ok(()) => (
            "success",
            tr.t("review.restored").to_string(),
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
        &reports_queue_url("review_restored"),
    )
}

/// POST /moderation/parking/{id}/invalidate — invalidate a location.
pub(crate) async fn moderation_parking_invalidate(
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
pub(crate) async fn moderation_parking_restore(
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
}
