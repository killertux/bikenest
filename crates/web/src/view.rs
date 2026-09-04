//! View models: translate application/domain types into presentation-ready
//! data for Askama templates (labels, formatting, CSS classes).
//!
//! Business rules stay in the application/domain layers; this module only
//! formats, maps and localizes (§7). Every user-facing label goes through the
//! request's [`Translator`] (§12).

use crate::PhotoVm;
use crate::i18n::Translator;
use bikenest_application::{
    AuthenticatedUser, GeoHit, ObjectStorage, ParkingSummary, PendingPhoto,
};
use bikenest_domain::{
    AccountState, Cost, FreshnessCategory, OpenStatus, OpeningHours, ParkingType, PricingUnit, Role,
};
use chrono::{Datelike, Timelike};
use std::time::Duration;

/// TTL for presigned photo GET URLs rendered into a page (S3-presign parity).
pub const PHOTO_URL_TTL: Duration = Duration::from_secs(3600);

/// JSON escaped so it can be embedded verbatim in a `<script type="application/json">`
/// (or an HTML attribute) without breaking out — the stored-XSS fix, §103.
///
/// `serde_json` does not escape `<`, `>`, `&`, U+2028 or U+2029, all of which can
/// terminate a `<script>` block or an attribute. Escaping them to `\uXXXX` keeps
/// `JSON.parse` (or the browser's JSON handling) decoding back the original value,
/// but a literal `</script><img …>` can no longer appear in the output.
pub fn escape_script_json(s: String) -> String {
    s.replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// Resolve a location's stored photo key to a presigned URL, if present.
pub async fn resolve_photo(storage: &dyn ObjectStorage, key: Option<&str>) -> Option<String> {
    let k = key?;
    storage.presigned_get(k, PHOTO_URL_TTL).await.ok()
}

/// Filterable parking-type codes (labels come from the translator).
pub const TYPE_CODES: &[&str] = &["rack", "parking_facility", "indoor", "secured", "locker"];

/// The security catalog codes (labels come from the translator). Canonical list
/// lives in the domain (`SECURITY_FEATURE_CODES`).
use bikenest_domain::SECURITY_FEATURE_CODES as SECURITY_CODES;

/// One checkbox/radio option with its label and checked-state, precomputed in
/// Rust (Askama templates stay logic-light).
#[derive(Debug, Clone)]
pub struct OptionVm {
    pub value: &'static str,
    pub label: &'static str,
    pub checked: bool,
}

pub fn type_options(t: Translator, selected: Option<&str>) -> Vec<OptionVm> {
    let sel = selected.unwrap_or("");
    TYPE_CODES
        .iter()
        .map(|code| OptionVm {
            value: code,
            label: type_label_for_code(t, code),
            checked: sel.split(',').any(|c| c.trim() == *code),
        })
        .collect()
}

pub fn security_options(t: Translator, selected: Option<&str>) -> Vec<OptionVm> {
    let sel = selected.unwrap_or("");
    SECURITY_CODES
        .iter()
        .map(|code| OptionVm {
            value: code,
            label: t.security(code),
            checked: sel.split(',').any(|c| c.trim() == *code),
        })
        .collect()
}

fn type_label_for_code(t: Translator, code: &str) -> &'static str {
    match code {
        "rack" => t.t("type.rack"),
        "parking_facility" => t.t("type.parking_facility"),
        "indoor" => t.t("type.indoor"),
        "secured" => t.t("type.secured"),
        "locker" => t.t("type.locker"),
        _ => t.t("type.other"),
    }
}

pub fn type_label(t: Translator, ty: ParkingType) -> &'static str {
    match ty {
        ParkingType::Rack => t.t("type.rack"),
        ParkingType::ParkingFacility => t.t("type.parking_facility"),
        ParkingType::Indoor => t.t("type.indoor"),
        ParkingType::Secured => t.t("type.secured"),
        ParkingType::Locker => t.t("type.locker"),
        ParkingType::Other => t.t("type.other"),
    }
}

pub fn distance_label(m: f64) -> String {
    if m < 1000.0 {
        format!("{m:.0} m")
    } else {
        format!("{:.1} km", m / 1000.0)
    }
}

fn currency_symbol(code: &str) -> &str {
    match code {
        "BRL" => "R$",
        "EUR" => "€",
        "USD" => "$",
        other => other,
    }
}

pub fn cost_label(t: Translator, cost: &Cost) -> String {
    match cost {
        Cost::Free => t.t("cost.free").to_string(),
        Cost::Unknown => t.t("cost.unknown").to_string(),
        Cost::Paid { price: None } => t.t("cost.paid_unknown").to_string(),
        Cost::Paid { price: Some(money) } => {
            let major = money.cents() as f64 / 100.0;
            let unit = match money.unit() {
                PricingUnit::Hour => t.t("unit.hour"),
                PricingUnit::Day => t.t("unit.day"),
                PricingUnit::Month => t.t("unit.month"),
                PricingUnit::Entry => t.t("unit.entry"),
            };
            format!(
                "{} {} / {unit}",
                currency_symbol(money.currency().as_str()),
                format_money(t, major)
            )
        }
    }
}

/// Locale-aware decimal formatting (§116.6): pt-BR uses a comma as the decimal
/// separator (`1234,56`), en uses a dot. Thousands separators are not added.
pub fn format_money(t: Translator, value: f64) -> String {
    let s = format!("{value:.2}");
    if t.is_pt() { s.replace('.', ",") } else { s }
}

pub fn rating_label(t: Translator, avg: Option<f64>, count: i64) -> String {
    match avg {
        Some(a) => format!("{a:.1} ({count})"),
        None => t.t("rating.none").to_string(),
    }
}

pub fn freshness_label(t: Translator, f: FreshnessCategory) -> &'static str {
    match f {
        FreshnessCategory::Fresh => t.t("freshness.fresh"),
        FreshnessCategory::RecentlyVerified => t.t("freshness.recently_verified"),
        FreshnessCategory::Aging => t.t("freshness.aging"),
        FreshnessCategory::Stale => t.t("freshness.stale"),
        FreshnessCategory::VeryStale => t.t("freshness.very_stale"),
        FreshnessCategory::Never => t.t("freshness.never"),
    }
}

pub fn open_label(t: Translator, s: OpenStatus) -> &'static str {
    match s {
        OpenStatus::Open => t.t("open.now"),
        OpenStatus::Closed => t.t("open.closed"),
        OpenStatus::Unknown => t.t("open.unknown"),
    }
}

/// One parking card in the results list (P2 search results) and the map JSON.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CardVm {
    pub id: i64,
    pub name: String,
    pub address: String,
    pub type_label: String,
    pub cost_label: String,
    pub distance_label: String,
    pub rating_label: String,
    pub has_rating: bool,
    pub freshness_code: &'static str,
    pub freshness_label: &'static str,
    pub open_label: &'static str,
    pub is_open_now: bool,
    /// Up to 3 confirmed security attribute labels.
    pub security_chips: Vec<String>,
    /// True when the location has no confirmed security attributes.
    pub security_unknown: bool,
    pub url: String,
    pub lat: f64,
    pub lon: f64,
    /// Presigned URL of the location's own primary photo (object storage), when
    /// one exists; the template prefers this over the positional `image`.
    pub photo_url: Option<String>,
    /// Illustrative fallback photo (per type) when a location has no photo yet.
    pub image: &'static str,
    pub image_alt: &'static str,
}

impl CardVm {
    pub fn from_summary(
        t: Translator,
        s: &ParkingSummary,
        freshness: FreshnessCategory,
        photo_url: Option<String>,
    ) -> Self {
        let (image, image_alt) = image_for(s.parking_type);
        let security_chips: Vec<String> = s
            .security_yes
            .iter()
            .take(3)
            .map(|c| t.security(c).to_string())
            .filter(|l| !l.is_empty())
            .collect();
        Self {
            id: s.id,
            name: s.name.clone(),
            address: s.address.clone(),
            type_label: type_label(t, s.parking_type).to_string(),
            cost_label: cost_label(t, &s.cost),
            distance_label: distance_label(s.distance_m),
            rating_label: rating_label(t, s.rating.avg(), s.rating.count()),
            has_rating: s.rating.avg().is_some(),
            freshness_code: freshness.as_code(),
            freshness_label: freshness_label(t, freshness),
            open_label: open_label(
                t,
                if s.is_open_now {
                    OpenStatus::Open
                } else {
                    OpenStatus::Closed
                },
            ),
            is_open_now: s.is_open_now,
            security_unknown: security_chips.is_empty(),
            security_chips,
            url: format!("/parking/{}", s.id),
            lat: s.point.lat(),
            lon: s.point.lon(),
            photo_url,
            image,
            image_alt,
        }
    }
}

/// Deterministic fallback photo per parking type (photos from the approved
/// design export). Used only when a location has no photo of its own.
fn image_for(ty: ParkingType) -> (&'static str, &'static str) {
    match ty {
        ParkingType::Rack => (
            "/static/img/street-rack-mint-bike.jpg",
            "Mint-green city bicycle locked to a street bike rack",
        ),
        ParkingType::ParkingFacility | ParkingType::Indoor | ParkingType::Secured => (
            "/static/img/square-bike-rows.jpg",
            "Row of bicycles parked under trees in a sunny public square",
        ),
        ParkingType::Locker | ParkingType::Other => (
            "/static/img/mtb-pair-rack.jpg",
            "Two mountain bikes resting on a simple metal bike rack",
        ),
    }
}

/// Shared results payload used by both the full page and the HTMX fragment.
#[derive(Debug, Clone)]
pub struct ResultsData {
    pub destination_label: Option<String>,
    pub total_label: String,
    pub items: Vec<CardVm>,
    pub cursor_url: Option<String>,
    pub error: Option<String>,
    /// Precomputed JSON for the map (Alpine/JS reads this).
    pub map_json: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn build_results(
    t: Translator,
    page: &bikenest_application::SearchPage,
    hit: Option<&GeoHit>,
    destination_label: Option<String>,
    query_string: String,
    now: chrono::DateTime<chrono::Utc>,
    thresholds: &bikenest_domain::FreshnessThresholds,
    storage: &dyn ObjectStorage,
) -> ResultsData {
    let mut items = Vec::with_capacity(page.items.len());
    for s in &page.items {
        let freshness = bikenest_domain::categorize(s.last_verified_at, now, thresholds);
        let photo_url = resolve_photo(storage, s.photo_key.as_deref()).await;
        items.push(CardVm::from_summary(t, s, freshness, photo_url));
    }

    let map_json = escape_script_json(
        serde_json::json!({
            "origin": hit.map(|h| serde_json::json!({"lat": h.point.lat(), "lon": h.point.lon(), "label": h.label})),
            "items": items,
        })
        .to_string(),
    );

    // `query_string` carries no leading "?" — it is the joined parameters, so
    // the cursor is appended to a query string this function opens itself.
    let cursor_url = page.next_cursor.as_ref().map(|c| {
        let cursor = c.encode();
        if query_string.is_empty() {
            format!("/search?cursor={cursor}")
        } else {
            format!("/search?{query_string}&cursor={cursor}")
        }
    });

    ResultsData {
        destination_label,
        total_label: t.spots(page.total),
        items,
        cursor_url,
        error: None,
        map_json,
    }
}

/// One row of the weekly hours table on the details page.
pub struct HoursRowVm {
    pub day: &'static str,
    pub label: String,
    pub is_today: bool,
}

pub fn hours_rows(
    t: Translator,
    hours: &OpeningHours,
    tz: chrono_tz::Tz,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<HoursRowVm> {
    const DAY_KEYS: [&str; 7] = [
        "day.mon", "day.tue", "day.wed", "day.thu", "day.fri", "day.sat", "day.sun",
    ];
    let today = now.with_timezone(&tz).weekday().number_from_monday() as usize; // 1..=7
    let rows = hours.rows_by_day();
    (1..=7)
        .map(|d| {
            let ranges = &rows[d - 1];
            let label = if hours.is_unknown() {
                t.t("hours.unknown").to_string()
            } else if ranges.is_empty() {
                t.t("hours.closed").to_string()
            } else if ranges.iter().any(|r| r.all_day) {
                t.t("hours.all_day").to_string()
            } else {
                ranges
                    .iter()
                    .map(|r| {
                        format!(
                            "{:02}:{:02} – {:02}:{:02}",
                            r.opens_at.hour(),
                            r.opens_at.minute(),
                            r.closes_at.hour(),
                            r.closes_at.minute()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            HoursRowVm {
                day: t.t(DAY_KEYS[d - 1]),
                label,
                is_today: d == today,
            }
        })
        .collect()
}

/// Localized role label.
pub fn role_label(t: Translator, role: Role) -> &'static str {
    match role {
        Role::User => t.t("role.user"),
        Role::Moderator => t.t("role.moderator"),
        Role::Admin => t.t("role.admin"),
    }
}

/// Localized account-state label (C1 / M5).
pub fn account_state_label(t: Translator, s: AccountState) -> &'static str {
    match s {
        AccountState::PendingEmailVerification => t.t("account.state.pending"),
        AccountState::Active => t.t("account.state.active"),
        AccountState::Suspended => t.t("account.state.suspended"),
        AccountState::Deleted => t.t("account.state.deleted"),
    }
}

/// One row of the admin user-management table (M5).
#[derive(Debug, Clone)]
pub struct AdminUserVm {
    pub id: i64,
    pub email: String,
    pub roles_label: String,
    pub state_label: &'static str,
    /// Account-state code (ACTIVE/SUSPENDED/…) for conditional rendering.
    pub state: &'static str,
    pub is_verified: bool,
    pub has_moderator: bool,
    pub has_admin: bool,
}

// ---------------------------------------------------------------------------
// Community (M3) view models
// ---------------------------------------------------------------------------

/// One rendered review (D3 / P3). `photos` are the review's APPROVED photos
/// (§38), already resolved to presigned URLs for the card's thumbnails.
#[derive(Debug, Clone)]
pub struct ReviewVm {
    pub id: i64,
    pub rating: u8,
    pub stars: String,
    pub body: String,
    pub created_label: String,
    pub is_own: bool,
    pub photos: Vec<PhotoVm>,
}

pub fn review_vm(
    t: Translator,
    r: &bikenest_application::Review,
    is_own: bool,
    photos: Vec<PhotoVm>,
) -> ReviewVm {
    let stars = "★".repeat(r.rating.value() as usize);
    let created_label = time_ago_label(t, r.created_at);
    ReviewVm {
        id: r.id,
        rating: r.rating.value(),
        stars,
        body: r.body.as_str().to_string(),
        created_label,
        is_own,
        photos,
    }
}

fn time_ago_label(t: Translator, at: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let days = (now - at).num_days();
    if days == 0 {
        t.t("time.today").to_string()
    } else if days == 1 {
        t.t("time.yesterday").to_string()
    } else if days < 30 {
        t.t("time.days_ago").replace("{n}", &days.to_string())
    } else {
        let months = days / 30;
        t.t("time.months_ago").replace("{n}", &months.to_string())
    }
}

/// Build the admin user list as presentation-ready rows.
pub fn admin_users(t: Translator, users: &[AuthenticatedUser]) -> Vec<AdminUserVm> {
    users
        .iter()
        .map(|u| {
            let mut roles = u.roles.clone();
            roles.sort();
            roles.dedup();
            AdminUserVm {
                id: u.id.0,
                email: u.email.to_string(),
                roles_label: roles
                    .iter()
                    .map(|r| role_label(t, *r))
                    .collect::<Vec<_>>()
                    .join(", "),
                state_label: account_state_label(t, u.account_state),
                state: u.account_state.as_code(),
                is_verified: u.is_verified,
                has_moderator: u.has_role(Role::Moderator),
                has_admin: u.has_role(Role::Admin),
            }
        })
        .collect()
}

/// Localized confidence label (P3 confidence badge, §106).
pub fn confidence_label(t: Translator, c: bikenest_domain::Confidence) -> &'static str {
    match c {
        bikenest_domain::Confidence::Reported => t.t("confidence.reported"),
        bikenest_domain::Confidence::Verified => t.t("confidence.verified"),
        bikenest_domain::Confidence::RecentlyVerified => t.t("confidence.recently_verified"),
        bikenest_domain::Confidence::Stale => t.t("confidence.stale"),
        bikenest_domain::Confidence::Conflicting => t.t("confidence.conflicting"),
    }
}

/// One rendered confidence badge.
#[derive(Debug, Clone)]
pub struct ConfidenceVm {
    pub code: &'static str,
    pub label: &'static str,
}

pub fn confidence_vm(t: Translator, c: bikenest_domain::Confidence) -> ConfidenceVm {
    ConfidenceVm {
        code: c.as_code(),
        label: confidence_label(t, c),
    }
}

/// One rendered attribution dispute tally (§106).
#[derive(Debug, Clone)]
pub struct AttrDisputeVm {
    pub label: &'static str,
    pub incorrect: i64,
}

pub fn attr_dispute_vm(t: Translator, code: &str, incorrect: i64) -> AttrDisputeVm {
    AttrDisputeVm {
        label: attribute_label(t, code),
        incorrect,
    }
}

fn attribute_label(t: Translator, code: &str) -> &'static str {
    match code {
        "name" => t.t("attr.name"),
        "address" => t.t("attr.address"),
        "type" => t.t("attr.type"),
        "cost" => t.t("attr.cost"),
        "hours" => t.t("attr.hours"),
        "security" => t.t("attr.security"),
        "location" => t.t("attr.location"),
        _ => t.t("attr.unknown"),
    }
}

/// One rendered "recommended because…" reason (§105).
#[derive(Debug, Clone)]
pub struct ReasonVm {
    pub label: &'static str,
    pub detail: String,
}

pub fn reason_vm(t: Translator, r: &bikenest_application::Reason) -> ReasonVm {
    ReasonVm {
        label: t.t(r.label_key),
        detail: r.detail.clone(),
    }
}

/// One rendered row of the C5 contribution history feed.
#[derive(Debug, Clone)]
pub struct ContributionVm {
    pub kind_label: &'static str,
    pub target: String,
    pub state_label: &'static str,
    pub at_label: String,
}

/// One advisory duplicate candidate (D1/§36).
#[derive(Debug, Clone)]
pub struct DuplicateVm {
    pub id: i64,
    pub name: String,
    pub distance_label: String,
    pub similarity_label: String,
}

pub fn duplicate_vm(_t: Translator, d: &bikenest_application::DuplicateCandidate) -> DuplicateVm {
    DuplicateVm {
        id: d.id,
        name: d.name.clone(),
        distance_label: distance_label(d.distance_m),
        similarity_label: format!("{:.0}%", d.similarity * 100.0),
    }
}

/// One photo in the moderator queue (M2 screen). Includes the presigned URL of
/// the *processed derivative* (exactly what would publish), a small preview and
/// an anonymized "Contributor #id" label — never an email/OAuth subject (§80).
#[derive(Debug, Clone)]
pub struct ModerationPhotoVm {
    pub id: i64,
    pub kind: &'static str,
    pub location_id: i64,
    pub location_name: String,
    pub full_url: String,
    pub thumb_url: Option<String>,
    pub alt: String,
    pub dimensions: Option<String>,
    pub contributor_label: String,
    pub uploaded_label: String,
}

pub async fn moderation_photo_vm(
    t: Translator,
    storage: &dyn ObjectStorage,
    p: &PendingPhoto,
) -> ModerationPhotoVm {
    let full_url = resolve_photo(storage, Some(&p.storage_key))
        .await
        .unwrap_or_default();
    let thumb_url = match p.thumbnail_key.as_deref() {
        Some(k) => resolve_photo(storage, Some(k)).await,
        None => None,
    };
    let alt = p
        .alt
        .clone()
        .unwrap_or_else(|| format!("Photo of {}", p.parent_name));
    let dimensions = match (p.width, p.height) {
        (Some(w), Some(h)) => Some(format!("{w} × {h}")),
        _ => None,
    };
    let contributor_label = p
        .uploader_id
        .map(|uid| format!("{} #{}", t.t("moderation.contributor"), uid.0))
        .unwrap_or_default();
    ModerationPhotoVm {
        id: p.id,
        kind: match p.kind {
            bikenest_application::PhotoKind::Parking => "parking",
            bikenest_application::PhotoKind::Review => "review",
        },
        location_id: p.parent_id,
        location_name: p.parent_name.clone(),
        full_url,
        thumb_url,
        alt,
        dimensions,
        contributor_label,
        uploaded_label: time_ago_label(t, p.created_at),
    }
}

// ---------------------------------------------------------------------------
// Moderation & reporting (M5) view models
// ---------------------------------------------------------------------------

/// One row of the M3 reports queue.
#[derive(Debug, Clone)]
pub struct ReportVm {
    pub id: i64,
    /// The submitting user's id (moderators may compare against the viewer to
    /// hide resolve/dismiss on one's own report); never rendered on public pages.
    /// `None` once the reporter's account is anonymized (M6).
    pub reporter_id: Option<i64>,
    pub target_type_label: &'static str,
    pub target_id: i64,
    pub reason_label: &'static str,
    pub description: String,
    pub state_code: &'static str,
    pub state_label: &'static str,
    /// Tailwind color token for the state badge ("fresh" | "aging" | "stale").
    pub state_color: &'static str,
    pub reporter_label: String,
    pub claimed_by_label: String,
    pub created_label: String,
}

/// The report-reason option list (value = code, label = i18n) for the modal/select.
use bikenest_domain::REPORT_REASONS;

pub fn report_reason_options(t: Translator) -> Vec<OptionVm> {
    REPORT_REASONS
        .iter()
        .map(|code| OptionVm {
            value: code,
            label: report_reason_label(t, code),
            checked: false,
        })
        .collect()
}

fn report_target_label(t: Translator, code: &str) -> &'static str {
    match code {
        "parking" => t.t("report.target.parking"),
        "parking_photo" => t.t("report.target.parking_photo"),
        "review" => t.t("report.target.review"),
        "review_photo" => t.t("report.target.review_photo"),
        _ => t.t("report.target.other"),
    }
}

fn report_reason_label(t: Translator, reason: &str) -> &'static str {
    match reason {
        "nonexistent_parking" => t.t("report.reason.nonexistent_parking"),
        "incorrect_location" => t.t("report.reason.incorrect_location"),
        "incorrect_price" => t.t("report.reason.incorrect_price"),
        "incorrect_hours" => t.t("report.reason.incorrect_hours"),
        "incorrect_security" => t.t("report.reason.incorrect_security"),
        "duplicate" => t.t("report.reason.duplicate"),
        "inappropriate_photo" => t.t("report.reason.inappropriate_photo"),
        "inappropriate_review" => t.t("report.reason.inappropriate_review"),
        "spam" => t.t("report.reason.spam"),
        "abuse" => t.t("report.reason.abuse"),
        "other" => t.t("report.reason.other"),
        _ => t.t("report.reason.other"),
    }
}

fn report_state_label(t: Translator, s: bikenest_domain::ReportState) -> &'static str {
    match s {
        bikenest_domain::ReportState::Open => t.t("report.state.open"),
        bikenest_domain::ReportState::UnderReview => t.t("report.state.under_review"),
        bikenest_domain::ReportState::Resolved => t.t("report.state.resolved"),
        bikenest_domain::ReportState::Dismissed => t.t("report.state.dismissed"),
    }
}

pub fn report_vm(t: Translator, r: &bikenest_application::Report) -> ReportVm {
    ReportVm {
        id: r.id,
        reporter_id: r.reporter_id.map(|u| u.0),
        target_type_label: report_target_label(t, r.target_type.as_code()),
        target_id: r.target_id,
        reason_label: report_reason_label(t, &r.reason),
        description: r.description.clone().unwrap_or_default(),
        state_code: r.state.as_code(),
        state_label: report_state_label(t, r.state),
        state_color: match r.state {
            bikenest_domain::ReportState::Open => "danger",
            bikenest_domain::ReportState::UnderReview => "aging",
            bikenest_domain::ReportState::Resolved | bikenest_domain::ReportState::Dismissed => {
                "fresh"
            }
        },
        reporter_label: r
            .reporter_id
            .map(|u| format!("{} #{}", t.t("moderation.contributor"), u.0))
            .unwrap_or_else(|| t.t("report.reporter.anonymous").to_string()),
        claimed_by_label: r
            .claimed_by
            .map(|c| format!("{} #{}", t.t("moderation.moderator"), c.0))
            .unwrap_or_else(|| t.t("report.claimed.none").to_string()),
        created_label: time_ago_label(t, r.created_at),
    }
}

/// One row of the M4 proposal review queue.
#[derive(Debug, Clone)]
pub struct ProposalVm {
    pub id: i64,
    pub location_id: i64,
    pub location_name: String,
    /// Stable kind code ("move_location" | "change_existence") — templates must
    /// branch on this, never on the localized label.
    pub kind_code: &'static str,
    pub kind_label: &'static str,
    pub detail: String,
    pub proposer_label: String,
    pub base_version: i64,
    pub created_label: String,
}

fn proposal_kind_label(t: Translator, kind: bikenest_domain::ProposalKind) -> &'static str {
    match kind {
        bikenest_domain::ProposalKind::MoveLocation => t.t("proposal.kind.move"),
        bikenest_domain::ProposalKind::ChangeExistence => t.t("proposal.kind.existence"),
    }
}

fn proposal_detail(
    t: Translator,
    kind: bikenest_domain::ProposalKind,
    proposed: &serde_json::Value,
) -> String {
    match kind {
        bikenest_domain::ProposalKind::MoveLocation => {
            let lat = proposed.get("lat").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let lon = proposed.get("lon").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let tz = proposed
                .get("timezone")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            format!("{:.5}, {:.5} · {tz}", lat, lon)
        }
        bikenest_domain::ProposalKind::ChangeExistence => {
            let ex = proposed
                .get("existence")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if ex == "removed" {
                t.t("proposal.existence.removed").to_string()
            } else {
                t.t("proposal.existence.exists").to_string()
            }
        }
    }
}

pub fn proposal_vm(t: Translator, p: &bikenest_application::Proposal) -> ProposalVm {
    ProposalVm {
        id: p.id,
        location_id: p.location_id,
        location_name: p.location_name.clone(),
        kind_code: p.kind.as_code(),
        kind_label: proposal_kind_label(t, p.kind),
        detail: proposal_detail(t, p.kind, &p.proposed),
        proposer_label: p
            .proposer_id
            .map(|u| format!("{} #{}", t.t("moderation.proposer"), u.0))
            .unwrap_or_else(|| t.t("report.reporter.anonymous").to_string()),
        base_version: p.base_version,
        created_label: time_ago_label(t, p.created_at),
    }
}

/// One row of the admin audit-log viewer (M6). Metadata rendered as an escaped
/// JSON blob — by construction it carries no secrets/PII (§47).
#[derive(Debug, Clone)]
pub struct AuditRowVm {
    pub id: i64,
    pub actor_label: String,
    pub action: String,
    pub target_label: String,
    pub result_label: &'static str,
    pub metadata: String,
    pub created_label: String,
}

pub fn audit_row_vm(t: Translator, e: &bikenest_application::AuditStoredEvent) -> AuditRowVm {
    let actor_label = e
        .event
        .actor_user_id
        .map(|a| format!("{} #{}", t.t("moderation.actor"), a.0))
        .unwrap_or_else(|| t.t("audit.system").to_string());
    let result_label = if e.event.result == "success" {
        t.t("audit.result.success")
    } else {
        t.t("audit.result.failure")
    };
    AuditRowVm {
        id: e.id,
        actor_label,
        action: e.event.action.clone(),
        target_label: format!("{}:{}", e.event.target_type, e.event.target_id),
        result_label,
        metadata: e.event.metadata.to_string(),
        created_label: time_ago_label(t, e.created_at),
    }
}

// ---------------------------------------------------------------------------
// Privacy & account lifecycle (M6)
// ---------------------------------------------------------------------------

/// A locale-neutral ISO date-time label (YYYY-MM-DD HH:MM).
pub fn iso_datetime_label(t: Translator, dt: chrono::DateTime<chrono::Utc>) -> String {
    if t.is_pt() {
        dt.format("%d/%m/%Y %H:%M").to_string()
    } else {
        dt.format("%Y-%m-%d %H:%M").to_string()
    }
}

/// One row of the C7 export-status page (status + optional single-use link).
#[derive(Debug, Clone)]
pub struct ExportVm {
    pub id: i64,
    pub state_code: &'static str,
    pub state_label: &'static str,
    pub created_label: String,
    pub expires_label: String,
    pub download_token: Option<String>,
    pub is_ready: bool,
}

pub fn export_state_label(t: Translator, s: bikenest_domain::ExportState) -> &'static str {
    match s {
        bikenest_domain::ExportState::Ready => t.t("export.state.ready"),
        bikenest_domain::ExportState::Downloaded => t.t("export.state.downloaded"),
        bikenest_domain::ExportState::Expired => t.t("export.state.expired"),
    }
}

/// Build a C7 row. `token` is the single-use download token — only present for
/// the just-requested export (rendered once as the owner-only link).
pub fn export_vm(
    t: Translator,
    e: &bikenest_application::Export,
    token: Option<String>,
) -> ExportVm {
    ExportVm {
        id: e.id,
        state_code: e.state.as_code(),
        state_label: export_state_label(t, e.state),
        created_label: iso_datetime_label(t, e.created_at),
        expires_label: iso_datetime_label(t, e.expires_at),
        download_token: if e.state == bikenest_domain::ExportState::Ready {
            token
        } else {
            None
        },
        is_ready: e.state == bikenest_domain::ExportState::Ready,
    }
}

/// One row of the admin privacy-request queue (or the C6 rights list).
#[derive(Debug, Clone)]
pub struct PrivacyRequestVm {
    pub id: i64,
    pub kind_code: &'static str,
    pub kind_label: &'static str,
    pub state_code: &'static str,
    pub state_label: &'static str,
    pub created_label: String,
}

pub fn privacy_request_kind_label(
    t: Translator,
    kind: bikenest_domain::PrivacyRequestKind,
) -> &'static str {
    match kind {
        bikenest_domain::PrivacyRequestKind::Access => t.t("privacy.kind.access"),
        bikenest_domain::PrivacyRequestKind::Rectification => t.t("privacy.kind.rectification"),
        bikenest_domain::PrivacyRequestKind::Deletion => t.t("privacy.kind.deletion"),
        bikenest_domain::PrivacyRequestKind::Export => t.t("privacy.kind.export"),
        bikenest_domain::PrivacyRequestKind::Restriction => t.t("privacy.kind.restriction"),
        bikenest_domain::PrivacyRequestKind::Objection => t.t("privacy.kind.objection"),
        bikenest_domain::PrivacyRequestKind::ConsentWithdrawal => t.t("privacy.kind.consent"),
    }
}

pub fn privacy_request_state_label(
    t: Translator,
    s: bikenest_domain::PrivacyRequestState,
) -> &'static str {
    match s {
        bikenest_domain::PrivacyRequestState::Open => t.t("privacy.state.open"),
        bikenest_domain::PrivacyRequestState::InProgress => t.t("privacy.state.in_progress"),
        bikenest_domain::PrivacyRequestState::Completed => t.t("privacy.state.completed"),
        bikenest_domain::PrivacyRequestState::Declined => t.t("privacy.state.declined"),
    }
}

pub fn privacy_request_vm(
    t: Translator,
    r: &bikenest_application::PrivacyRequest,
) -> PrivacyRequestVm {
    PrivacyRequestVm {
        id: r.id,
        kind_code: r.kind.as_code(),
        kind_label: privacy_request_kind_label(t, r.kind),
        state_code: r.state.as_code(),
        state_label: privacy_request_state_label(t, r.state),
        created_label: iso_datetime_label(t, r.created_at),
    }
}

/// One row of the policy version-history list.
#[derive(Debug, Clone)]
pub struct PolicyVersionVm {
    pub version: String,
    pub effective_label: String,
    pub is_current: bool,
}

pub fn policy_version_vm(
    t: Translator,
    doc: &bikenest_application::PolicyDocument,
) -> PolicyVersionVm {
    PolicyVersionVm {
        version: doc.version.clone(),
        effective_label: iso_datetime_label(t, doc.effective_at),
        is_current: doc.superseded_at.is_none(),
    }
}

/// One selectable manual rights kind on the C6 hub.
#[derive(Debug, Clone)]
pub struct PrivacyRequestKindVm {
    pub code: &'static str,
    pub label: &'static str,
    pub description: &'static str,
}

/// The manual (operator-fulfilled) rights kinds, with descriptions, for the C6
/// request list (§72). Access/export and deletion are automatic and have their
/// own cards.
pub fn privacy_request_kind_options(t: Translator) -> Vec<PrivacyRequestKindVm> {
    vec![
        PrivacyRequestKindVm {
            code: "rectification",
            label: privacy_request_kind_label(
                t,
                bikenest_domain::PrivacyRequestKind::Rectification,
            ),
            description: t.t("privacy.rights.rectification_desc"),
        },
        PrivacyRequestKindVm {
            code: "restriction",
            label: privacy_request_kind_label(t, bikenest_domain::PrivacyRequestKind::Restriction),
            description: t.t("privacy.rights.restriction_desc"),
        },
        PrivacyRequestKindVm {
            code: "objection",
            label: privacy_request_kind_label(t, bikenest_domain::PrivacyRequestKind::Objection),
            description: t.t("privacy.rights.objection_desc"),
        },
        PrivacyRequestKindVm {
            code: "consent_withdrawal",
            label: privacy_request_kind_label(
                t,
                bikenest_domain::PrivacyRequestKind::ConsentWithdrawal,
            ),
            description: t.t("privacy.rights.consent_desc"),
        },
    ]
}

pub fn contribution_vm(
    t: Translator,
    i: &bikenest_application::ContributionItem,
) -> ContributionVm {
    let kind = match i.kind.as_str() {
        "added" => t.t("contrib.kind.added"),
        "edited" => t.t("contrib.kind.edited"),
        "proposed" => t.t("contrib.kind.proposed"),
        "reviewed" => t.t("contrib.kind.reviewed"),
        "verified" => t.t("contrib.kind.verified"),
        "favorited" => t.t("contrib.kind.favorited"),
        _ => t.t("contrib.kind.other"),
    };
    let state = match i.state.as_str() {
        "active" => t.t("contrib.state.active"),
        "pending" => t.t("contrib.state.pending"),
        "history" => t.t("contrib.state.history"),
        _ => t.t("contrib.state.other"),
    };
    ContributionVm {
        kind_label: kind,
        target: i.target.clone(),
        state_label: state,
        at_label: time_ago_label(t, i.at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Locale;
    use bikenest_application::{Cursor, SearchPage, Sort};
    use bikenest_test_support::TestObjectStorage;

    fn page_with_next() -> SearchPage {
        SearchPage {
            items: Vec::new(),
            total: 42,
            next_cursor: Some(Cursor {
                sort: Sort::Distance,
                v: 250.0,
                id: 7,
            }),
        }
    }

    async fn cursor_url_for(query_string: &str) -> Option<String> {
        let storage = TestObjectStorage::new();
        build_results(
            Translator::new(Locale::En),
            &page_with_next(),
            None,
            None,
            query_string.to_string(),
            chrono::Utc::now(),
            &bikenest_domain::DEFAULT_THRESHOLDS,
            &storage,
        )
        .await
        .cursor_url
    }

    /// The query string arrives without a leading "?" — the next-page link has
    /// to open the query itself, or the first parameter fuses onto the path.
    #[tokio::test]
    async fn next_page_url_keeps_the_current_query() {
        let url = cursor_url_for("q=rua&radius=1000")
            .await
            .expect("a next page exists");
        assert!(
            url.starts_with("/search?q=rua"),
            "query must open with '?': {url}"
        );
        assert!(url.contains("&radius=1000"), "filters are kept: {url}");
        assert!(url.contains("&cursor="), "cursor is appended: {url}");
    }

    #[tokio::test]
    async fn next_page_url_without_a_query_still_opens_the_query_string() {
        let url = cursor_url_for("").await.expect("a next page exists");
        assert!(url.starts_with("/search?cursor="), "{url}");
    }

    #[tokio::test]
    async fn no_next_cursor_means_no_next_page_link() {
        let storage = TestObjectStorage::new();
        let page = SearchPage {
            items: Vec::new(),
            total: 3,
            next_cursor: None,
        };
        let results = build_results(
            Translator::new(Locale::En),
            &page,
            None,
            None,
            "q=rua".to_string(),
            chrono::Utc::now(),
            &bikenest_domain::DEFAULT_THRESHOLDS,
            &storage,
        )
        .await;
        assert!(results.cursor_url.is_none());
    }
}
