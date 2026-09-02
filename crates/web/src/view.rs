//! View models: translate application/domain types into presentation-ready
//! data for Askama templates (labels, formatting, CSS classes).
//!
//! Business rules stay in the application/domain layers; this module only
//! formats, maps and localizes (§7). Every user-facing label goes through the
//! request's [`Translator`] (§12).

use crate::i18n::Translator;
use bikenest_application::{GeoHit, ObjectStorage, ParkingSummary};
use bikenest_domain::{Cost, FreshnessCategory, OpenStatus, OpeningHours, ParkingType, PricingUnit};
use chrono::{Datelike, Timelike};
use std::time::Duration;

/// TTL for presigned photo GET URLs rendered into a page (S3-presign parity).
pub const PHOTO_URL_TTL: Duration = Duration::from_secs(3600);

/// Resolve a location's stored photo key to a presigned URL, if present.
pub fn resolve_photo(storage: &dyn ObjectStorage, key: Option<&str>) -> Option<String> {
    key.and_then(|k| storage.presigned_get(k, PHOTO_URL_TTL).ok())
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
                "{} {major:.2} / {unit}",
                currency_symbol(money.currency().as_str())
            )
        }
    }
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

/// One parking card in the results list (UI_DESIGN P2) and the map JSON.
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
pub fn build_results(
    t: Translator,
    page: &bikenest_application::SearchPage,
    hit: Option<&GeoHit>,
    destination_label: Option<String>,
    query_string: String,
    now: chrono::DateTime<chrono::Utc>,
    storage: &dyn ObjectStorage,
) -> ResultsData {
    let items: Vec<CardVm> = page
        .items
        .iter()
        .map(|s| {
            let freshness = bikenest_domain::categorize(
                s.last_verified_at,
                now,
                &bikenest_domain::DEFAULT_THRESHOLDS,
            );
            let photo_url = resolve_photo(storage, s.photo_key.as_deref());
            CardVm::from_summary(t, s, freshness, photo_url)
        })
        .collect();

    let map_json = serde_json::json!({
        "origin": hit.map(|h| serde_json::json!({"lat": h.point.lat(), "lon": h.point.lon(), "label": h.label})),
        "items": items,
    })
    .to_string();

    let cursor_url = page.next_cursor.as_ref().map(|c| {
        let sep = if query_string.is_empty() { "?" } else { "&" };
        format!("/search{query_string}{sep}cursor={}", c.encode())
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
