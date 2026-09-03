//! Moderation & reporting domain (REQUIREMENTS §43–§45, §103).
//!
//! Pure, no I/O. Models the report lifecycle, the valid target types, the
//! reason code list and the per-target reason mapping. The audience for these
//! is the application [`ModerationService`](crate::application::moderation::ModerationService)
//! and the infrastructure repositories.

use crate::DomainError;

// ---------------------------------------------------------------------------
// Report state machine (§43)
// ---------------------------------------------------------------------------

/// Lifecycle of a report. `Claim` is the only `Open -> UnderReview` move;
/// `resolve`/`dismiss` are the terminal moves. No re-open in this milestone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportState {
    Open,
    UnderReview,
    Resolved,
    Dismissed,
}

impl ReportState {
    pub fn as_code(self) -> &'static str {
        match self {
            ReportState::Open => "OPEN",
            ReportState::UnderReview => "UNDER_REVIEW",
            ReportState::Resolved => "RESOLVED",
            ReportState::Dismissed => "DISMISSED",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, DomainError> {
        match code {
            "OPEN" => Ok(ReportState::Open),
            "UNDER_REVIEW" => Ok(ReportState::UnderReview),
            "RESOLVED" => Ok(ReportState::Resolved),
            "DISMISSED" => Ok(ReportState::Dismissed),
            other => Err(DomainError::Invalid(format!(
                "unknown report state: {other}"
            ))),
        }
    }
}

/// The terminal outcomes of a claimed report (§43).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportOutcome {
    /// The report was valid — the target was moderated (hidden/invalidated).
    Resolved,
    /// The report was unfounded — no action taken.
    Dismissed,
}

impl ReportOutcome {
    pub fn as_code(self) -> &'static str {
        match self {
            ReportOutcome::Resolved => "RESOLVED",
            ReportOutcome::Dismissed => "DISMISSED",
        }
    }
}

// ---------------------------------------------------------------------------
// Report target type (§43)
// ---------------------------------------------------------------------------

/// The four UI_DESIGN "target content" kinds a report may address. A single
/// `photo` type would collide across `parking_photo`/`review_photo`, so the
/// four-way type keeps resolution unambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReportTargetType {
    Parking,
    ParkingPhoto,
    Review,
    ReviewPhoto,
}

impl ReportTargetType {
    pub fn as_code(self) -> &'static str {
        match self {
            ReportTargetType::Parking => "parking",
            ReportTargetType::ParkingPhoto => "parking_photo",
            ReportTargetType::Review => "review",
            ReportTargetType::ReviewPhoto => "review_photo",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, DomainError> {
        match code {
            "parking" => Ok(ReportTargetType::Parking),
            "parking_photo" => Ok(ReportTargetType::ParkingPhoto),
            "review" => Ok(ReportTargetType::Review),
            "review_photo" => Ok(ReportTargetType::ReviewPhoto),
            other => Err(DomainError::Invalid(format!(
                "unknown report target type: {other}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Report reasons (§43)
// ---------------------------------------------------------------------------

/// The eleven allowed report reasons (§43). Labels are localized in the
/// presentation layer (i18n), so the catalog is a hardcoded code list here —
/// adding a reason is a code + translation, no migration (mirrors §28).
pub const REPORT_REASONS: &[&str] = &[
    "nonexistent_parking",
    "incorrect_location",
    "incorrect_price",
    "incorrect_hours",
    "incorrect_security",
    "duplicate",
    "inappropriate_photo",
    "inappropriate_review",
    "spam",
    "abuse",
    "other",
];

/// Whether `code` is a recognized report reason (§43).
pub fn is_known_report_reason(code: &str) -> bool {
    REPORT_REASONS.contains(&code)
}

/// Enforce §43's sensible mapping: which reasons may target which entity.
///
/// - `inappropriate_photo` only targets photo kinds.
/// - `inappropriate_review` only targets reviews.
/// - `duplicate` only targets a parking location (a photo/review is not a
///   duplicate listing).
/// - Everything else is allowed on any target type.
pub fn reason_allowed_for(target: ReportTargetType, reason: &str) -> bool {
    let is_photo = target == ReportTargetType::ParkingPhoto || target == ReportTargetType::ReviewPhoto;
    let is_review = target == ReportTargetType::Review || target == ReportTargetType::ReviewPhoto;
    match reason {
        "inappropriate_photo" => is_photo,
        "inappropriate_review" => is_review,
        "duplicate" => target == ReportTargetType::Parking,
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// ReportDescription (§103)
// ---------------------------------------------------------------------------

/// A report description, trimmed to `0..=1000` chars. Optional; whitespace-only
/// collapses to `None` (an empty string) — the caller keeps the field only when
/// the user typed something meaningful.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportDescription(String);

impl ReportDescription {
    pub const MAX_LEN: usize = 1000;

    /// Builds from a raw untrusted string. `Ok(None)` is not expressible here —
    /// the caller decides whether to store `Some` or `None` (it returns
    /// [`Self`] for a non-empty, in-range description). Uses [`Self::MAX_LEN`].
    pub fn new(raw: &str) -> Result<Self, DomainError> {
        Self::new_with_len(raw, Self::MAX_LEN)
    }

    /// Builds with a runtime-configured max length (a value ≤ [`Self::MAX_LEN`];
    /// the hard constant is the ceiling, the app may lower it, Ledger #19).
    pub fn new_with_len(raw: &str, max_len: usize) -> Result<Self, DomainError> {
        let trimmed = raw.trim();
        let len = trimmed.chars().count();
        if len > max_len {
            return Err(DomainError::Invalid(format!(
                "report description must be at most {max_len} characters: {len}"
            )));
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Runtime-tunable moderation limits (§43, Ledger #19). Default to the domain
/// constants; the application passes its configured value (env-driven) so
/// operators can tune without a rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModerationLimits {
    pub report_description_max_len: usize,
    pub report_create_user_limit: u32,
    pub report_create_ip_limit: u32,
}

impl Default for ModerationLimits {
    fn default() -> Self {
        Self {
            report_description_max_len: ReportDescription::MAX_LEN,
            report_create_user_limit: 10,
            report_create_ip_limit: 20,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_state_codes_round_trip() {
        for s in [
            ReportState::Open,
            ReportState::UnderReview,
            ReportState::Resolved,
            ReportState::Dismissed,
        ] {
            assert_eq!(ReportState::from_code(s.as_code()), Ok(s));
        }
        assert!(ReportState::from_code("CLOSED").is_err());
        assert!(ReportState::from_code("open").is_err());
    }

    #[test]
    fn report_target_types_round_trip() {
        for t in [
            ReportTargetType::Parking,
            ReportTargetType::ParkingPhoto,
            ReportTargetType::Review,
            ReportTargetType::ReviewPhoto,
        ] {
            assert_eq!(ReportTargetType::from_code(t.as_code()), Ok(t));
        }
        assert!(ReportTargetType::from_code("location").is_err());
    }

    #[test]
    fn all_report_reasons_are_known() {
        for r in REPORT_REASONS {
            assert!(is_known_report_reason(r), "{r}");
        }
        assert!(!is_known_report_reason("too_expensive"));
    }

    #[test]
    fn reason_allowed_for_mapping_boundaries() {
        use ReportTargetType::*;
        // inappropriate_photo on photo targets only.
        assert!(reason_allowed_for(ParkingPhoto, "inappropriate_photo"));
        assert!(reason_allowed_for(ReviewPhoto, "inappropriate_photo"));
        assert!(!reason_allowed_for(Parking, "inappropriate_photo"));
        assert!(!reason_allowed_for(Review, "inappropriate_photo"));

        // inappropriate_review on review targets only.
        assert!(reason_allowed_for(Review, "inappropriate_review"));
        assert!(reason_allowed_for(ReviewPhoto, "inappropriate_review"));
        assert!(!reason_allowed_for(Parking, "inappropriate_review"));
        assert!(!reason_allowed_for(ParkingPhoto, "inappropriate_review"));

        // duplicate on parking only.
        assert!(reason_allowed_for(Parking, "duplicate"));
        assert!(!reason_allowed_for(Review, "duplicate"));

        // Generic reasons allowed anywhere.
        for t in [Parking, ParkingPhoto, Review, ReviewPhoto] {
            assert!(reason_allowed_for(t, "spam"));
            assert!(reason_allowed_for(t, "abuse"));
            assert!(reason_allowed_for(t, "other"));
            assert!(reason_allowed_for(t, "nonexistent_parking"));
        }
    }

    #[test]
    fn report_description_trim_and_length() {
        let d = ReportDescription::new("  Duplicate listing  ").unwrap();
        assert_eq!(d.as_str(), "Duplicate listing");

        // Whitespace only collapses to empty (in range).
        assert_eq!(ReportDescription::new("   ").unwrap().as_str(), "");

        // At the max is fine; one past → error.
        assert!(ReportDescription::new(&"x".repeat(1000)).is_ok());
        assert!(ReportDescription::new(&"x".repeat(1001)).is_err());
    }
}
