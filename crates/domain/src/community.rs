//! Community contribution domain (REQUIREMENTS §37–§42, §106).
//!
//! Pure, no I/O. Models the value objects and the confidence-resolution rule
//! for the M3 write flows (add/edit/propose/review/verify/favorite). Persistence
//! and orchestration live in the application / infrastructure layers.

use crate::{DomainError, FreshnessThresholds, UserId};
use chrono::{DateTime, Utc};

// ---------------------------------------------------------------------------
// StarRating (§38)
// ---------------------------------------------------------------------------

/// A five-star rating, validated to `1..=5`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StarRating(u8);

impl StarRating {
    pub fn new(value: u8) -> Result<Self, DomainError> {
        if !(1..=5).contains(&value) {
            return Err(DomainError::Invalid(format!(
                "rating must be between 1 and 5: {value}"
            )));
        }
        Ok(Self(value))
    }

    /// From a DB `SMALLINT`. Frees the repository from checking bounds.
    pub fn from_smallint(value: i16) -> Result<Self, DomainError> {
        if !(1..=5).contains(&value) {
            return Err(DomainError::Invalid(format!(
                "rating must be between 1 and 5: {value}"
            )));
        }
        Ok(Self(value as u8))
    }

    pub fn value(self) -> u8 {
        self.0
    }

    pub fn as_i16(self) -> i16 {
        i16::from(self.0)
    }
}

// ---------------------------------------------------------------------------
// ReviewBody (§38)
// ---------------------------------------------------------------------------

/// A review body, trimmed to `1..=2000` chars.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewBody(String);

impl ReviewBody {
    pub fn new(raw: &str) -> Result<Self, DomainError> {
        let trimmed = raw.trim();
        let len = trimmed.chars().count();
        if !(1..=2000).contains(&len) {
            return Err(DomainError::Invalid(format!(
                "review body must be 1..=2000 characters: {len}"
            )));
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// Verification kinds / results (§39)
// ---------------------------------------------------------------------------

/// The kind of a verification signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VerificationKind {
    /// A rider confirms whether the location still exists / matches the listing.
    Existence,
    /// A rider confirms or disputes one attribute (name/address/type/…).
    Attribute,
    /// A rider "parked here" — a private, short-lived usage signal (§41).
    ParkedHere,
}

impl VerificationKind {
    pub fn as_code(&self) -> &'static str {
        match self {
            VerificationKind::Existence => "existence",
            VerificationKind::Attribute => "attribute",
            VerificationKind::ParkedHere => "parked_here",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, DomainError> {
        match code {
            "existence" => Ok(VerificationKind::Existence),
            "attribute" => Ok(VerificationKind::Attribute),
            "parked_here" => Ok(VerificationKind::ParkedHere),
            other => Err(DomainError::Invalid(format!(
                "unknown verification kind: {other}"
            ))),
        }
    }
}

/// The result of an *existence* verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExistenceResult {
    /// Confirmed still exists as listed.
    StillExists,
    /// Confirmed the location is gone.
    NoLongerExists,
    /// Exists, but some information is now wrong.
    InfoChanged,
}

impl ExistenceResult {
    pub fn as_code(&self) -> &'static str {
        match self {
            ExistenceResult::StillExists => "still_exists",
            ExistenceResult::NoLongerExists => "no_longer_exists",
            ExistenceResult::InfoChanged => "info_changed",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, DomainError> {
        match code {
            "still_exists" => Ok(ExistenceResult::StillExists),
            "no_longer_exists" => Ok(ExistenceResult::NoLongerExists),
            "info_changed" => Ok(ExistenceResult::InfoChanged),
            other => Err(DomainError::Invalid(format!(
                "unknown existence result: {other}"
            ))),
        }
    }
}

/// The result of an *attribute* verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttributeResult {
    Correct,
    Incorrect,
}

impl AttributeResult {
    pub fn as_code(&self) -> &'static str {
        match self {
            AttributeResult::Correct => "correct",
            AttributeResult::Incorrect => "incorrect",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, DomainError> {
        match code {
            "correct" => Ok(AttributeResult::Correct),
            "incorrect" => Ok(AttributeResult::Incorrect),
            other => Err(DomainError::Invalid(format!(
                "unknown attribute result: {other}"
            ))),
        }
    }
}

/// The per-attribute codes that an *attribute* verification may target (§39).
pub const ATTRIBUTE_CODES: &[&str] = &[
    "name", "address", "type", "cost", "hours", "security", "location",
];

/// Whether `code` is a recognized attribute target (§39).
pub fn is_known_attribute_code(code: &str) -> bool {
    ATTRIBUTE_CODES.contains(&code)
}

// ---------------------------------------------------------------------------
// Proposals (§37/§107)
// ---------------------------------------------------------------------------

/// The kind of gated, sensitive change a rider can propose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProposalKind {
    /// Move the pin / change the coordinates or timezone.
    MoveLocation,
    /// Change the existence (e.g. propose removal).
    ChangeExistence,
}

impl ProposalKind {
    pub fn as_code(&self) -> &'static str {
        match self {
            ProposalKind::MoveLocation => "move_location",
            ProposalKind::ChangeExistence => "change_existence",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, DomainError> {
        match code {
            "move_location" => Ok(ProposalKind::MoveLocation),
            "change_existence" => Ok(ProposalKind::ChangeExistence),
            other => Err(DomainError::Invalid(format!(
                "unknown proposal kind: {other}"
            ))),
        }
    }
}

/// Lifecycle of a proposal. M3 creates `Pending`; resolution is M5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProposalStatus {
    Pending,
    Approved,
    Rejected,
    Superseded,
}

impl ProposalStatus {
    pub fn as_code(&self) -> &'static str {
        match self {
            ProposalStatus::Pending => "PENDING",
            ProposalStatus::Approved => "APPROVED",
            ProposalStatus::Rejected => "REJECTED",
            ProposalStatus::Superseded => "SUPERSEDED",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, DomainError> {
        match code {
            "PENDING" => Ok(ProposalStatus::Pending),
            "APPROVED" => Ok(ProposalStatus::Approved),
            "REJECTED" => Ok(ProposalStatus::Rejected),
            "SUPERSEDED" => Ok(ProposalStatus::Superseded),
            other => Err(DomainError::Invalid(format!(
                "unknown proposal status: {other}"
            ))),
        }
    }
}

/// The kind of a `parking_revision` row (field-level history, §107).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Create,
    Edit,
    Moderation,
    Verification,
}

impl ChangeKind {
    pub fn as_code(&self) -> &'static str {
        match self {
            ChangeKind::Create => "create",
            ChangeKind::Edit => "edit",
            ChangeKind::Moderation => "moderation",
            ChangeKind::Verification => "verification",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, DomainError> {
        match code {
            "create" => Ok(ChangeKind::Create),
            "edit" => Ok(ChangeKind::Edit),
            "moderation" => Ok(ChangeKind::Moderation),
            "verification" => Ok(ChangeKind::Verification),
            other => Err(DomainError::Invalid(format!(
                "unknown change kind: {other}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Confidence (§106)
// ---------------------------------------------------------------------------

/// Computed, per-detail-read confidence in a location's reported existence and
/// freshness. Never denormalized; conflicts are surfaced, not averaged away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// Only the listing's own data — no community existence signal yet.
    Reported,
    /// Positive existence confirmation, not recently.
    Verified,
    /// Positive existence confirmation within the fresh window.
    RecentlyVerified,
    /// Positive existence confirmation that has since aged out.
    Stale,
    /// A community member reports the location no longer exists, contradicting
    /// the active listing. Never silently averaged.
    Conflicting,
}

impl Confidence {
    pub fn as_code(&self) -> &'static str {
        match self {
            Confidence::Reported => "reported",
            Confidence::Verified => "verified",
            Confidence::RecentlyVerified => "recently_verified",
            Confidence::Stale => "stale",
            Confidence::Conflicting => "conflicting",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "reported" => Some(Confidence::Reported),
            "verified" => Some(Confidence::Verified),
            "recently_verified" => Some(Confidence::RecentlyVerified),
            "stale" => Some(Confidence::Stale),
            "conflicting" => Some(Confidence::Conflicting),
            _ => None,
        }
    }
}

/// The latest existence verification per user, already deduped by the reader
/// (`DISTINCT ON (user_id) … ORDER BY user_id, created_at DESC`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistenceSignal {
    pub user: UserId,
    pub result: ExistenceResult,
    pub at: DateTime<Utc>,
}

impl ExistenceSignal {
    pub fn new(user: UserId, result: ExistenceResult, at: DateTime<Utc>) -> Self {
        Self { user, result, at }
    }
}

/// Resolve the [`Confidence`] of a location from its latest-per-user existence
/// signals (§106).
///
/// 1. No existence signals → [`Confidence::Reported`].
/// 2. Any `no_longer_exists` → [`Confidence::Conflicting`] (the DB says active;
///    the community says gone — never silently averaged).
/// 3. Otherwise (all positives are `still_exists`): classify the **latest**
///    `still_exists` by freshness → `Fresh` ⇒ `RecentlyVerified`;
///    `RecentlyVerified`/`Aging` ⇒ `Verified`; `Stale`/`VeryStale` ⇒ `Stale`.
///
/// `info_changed` (and per-attribute `incorrect`) do **not** change the enum;
/// they feed a separate `disputed` flag. `parked_here` is excluded entirely.
pub fn confidence(
    signals: &[ExistenceSignal],
    now: DateTime<Utc>,
    thresholds: &FreshnessThresholds,
) -> Confidence {
    if signals.is_empty() {
        return Confidence::Reported;
    }
    if signals
        .iter()
        .any(|s| s.result == ExistenceResult::NoLongerExists)
    {
        return Confidence::Conflicting;
    }
    // Only still_exists confirms existence. info_changed-only signals neither
    // confirm nor deny → stay Reported (the caller flags `disputed`).
    let positives: Vec<&ExistenceSignal> = signals
        .iter()
        .filter(|s| s.result == ExistenceResult::StillExists)
        .collect();
    let Some(latest) = positives.iter().max_by(|a, b| a.at.cmp(&b.at)) else {
        return Confidence::Reported;
    };
    let category = crate::freshness::categorize(Some(latest.at), now, thresholds);
    use crate::freshness::FreshnessCategory::*;
    match category {
        Fresh => Confidence::RecentlyVerified,
        RecentlyVerified | Aging => Confidence::Verified,
        Stale | VeryStale => Confidence::Stale,
        Never => Confidence::Reported,
    }
}

// ---------------------------------------------------------------------------
// ChangeKind / code round-trip helpers for history summaries
// ---------------------------------------------------------------------------

/// A short, human-readable change summary for C5 (localization happens at the
/// web boundary; this is a machine code + a generic label).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionSummary {
    pub version: i64,
    pub change_kind: ChangeKind,
    pub summary: Option<String>,
    pub at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn signal(
        user: i64,
        result: ExistenceResult,
        days_ago: i64,
        now: DateTime<Utc>,
    ) -> ExistenceSignal {
        ExistenceSignal::new(UserId(user), result, now - chrono::Duration::days(days_ago))
    }

    // --- StarRating / ReviewBody -----------------------------------------

    #[test]
    fn star_rating_boundaries() {
        assert_eq!(StarRating::new(1).unwrap().value(), 1);
        assert_eq!(StarRating::new(5).unwrap().value(), 5);
        assert!(StarRating::new(0).is_err());
        assert!(StarRating::new(6).is_err());
        assert_eq!(StarRating::from_smallint(3).unwrap().as_i16(), 3);
        assert!(StarRating::from_smallint(0).is_err());
    }

    #[test]
    fn review_body_trims_and_validates_length() {
        let body = ReviewBody::new("  Great rack near the station  ").unwrap();
        assert_eq!(body.as_str(), "Great rack near the station");
        assert!(ReviewBody::new("").is_err());
        assert!(ReviewBody::new("   ").is_err());
        let long = "x".repeat(2001);
        assert!(ReviewBody::new(&long).is_err());
        assert!(ReviewBody::new("ok").is_ok());
    }

    // --- code round-trips -------------------------------------------------

    #[test]
    fn verification_kind_round_trips() {
        for kind in [
            VerificationKind::Existence,
            VerificationKind::Attribute,
            VerificationKind::ParkedHere,
        ] {
            assert_eq!(VerificationKind::from_code(kind.as_code()).unwrap(), kind);
        }
        assert!(VerificationKind::from_code("photo").is_err());
    }

    #[test]
    fn existence_result_round_trips() {
        for r in [
            ExistenceResult::StillExists,
            ExistenceResult::NoLongerExists,
            ExistenceResult::InfoChanged,
        ] {
            assert_eq!(ExistenceResult::from_code(r.as_code()).unwrap(), r);
        }
        assert!(ExistenceResult::from_code("maybe").is_err());
    }

    #[test]
    fn attribute_result_round_trips() {
        assert_eq!(
            AttributeResult::from_code("correct").unwrap(),
            AttributeResult::Correct
        );
        assert_eq!(
            AttributeResult::from_code("incorrect").unwrap(),
            AttributeResult::Incorrect
        );
        assert!(AttributeResult::from_code("fine").is_err());
    }

    #[test]
    fn attribute_code_catalog_is_known() {
        for code in ATTRIBUTE_CODES {
            assert!(is_known_attribute_code(code), "{code}");
        }
        assert!(!is_known_attribute_code("photo"));
    }

    #[test]
    fn proposal_kind_and_status_round_trip() {
        for k in [ProposalKind::MoveLocation, ProposalKind::ChangeExistence] {
            assert_eq!(ProposalKind::from_code(k.as_code()).unwrap(), k);
        }
        for s in [
            ProposalStatus::Pending,
            ProposalStatus::Approved,
            ProposalStatus::Rejected,
            ProposalStatus::Superseded,
        ] {
            assert_eq!(ProposalStatus::from_code(s.as_code()).unwrap(), s);
        }
    }

    #[test]
    fn change_kind_round_trips() {
        for k in [
            ChangeKind::Create,
            ChangeKind::Edit,
            ChangeKind::Moderation,
            ChangeKind::Verification,
        ] {
            assert_eq!(ChangeKind::from_code(k.as_code()).unwrap(), k);
        }
    }

    // --- confidence rule --------------------------------------------------

    fn tz() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap()
    }

    #[test]
    fn confidence_empty_signals_is_reported() {
        assert_eq!(
            confidence(&[], tz(), &crate::freshness::DEFAULT_THRESHOLDS),
            Confidence::Reported
        );
    }

    #[test]
    fn confidence_positive_classifies_by_freshness() {
        let now = tz();
        let thresholds = crate::freshness::DEFAULT_THRESHOLDS;
        // Fresh (0 days ago) → RecentlyVerified.
        assert_eq!(
            confidence(
                &[signal(1, ExistenceResult::StillExists, 0, now)],
                now,
                &thresholds
            ),
            Confidence::RecentlyVerified
        );
        // RecentlyVerified (45 days) → Verified.
        assert_eq!(
            confidence(
                &[signal(1, ExistenceResult::StillExists, 45, now)],
                now,
                &thresholds
            ),
            Confidence::Verified
        );
        // Aging (120 days) → Verified.
        assert_eq!(
            confidence(
                &[signal(1, ExistenceResult::StillExists, 120, now)],
                now,
                &thresholds
            ),
            Confidence::Verified
        );
        // Stale (200 days) → Stale.
        assert_eq!(
            confidence(
                &[signal(1, ExistenceResult::StillExists, 200, now)],
                now,
                &thresholds
            ),
            Confidence::Stale
        );
        // VeryStale (400 days) → Stale.
        assert_eq!(
            confidence(
                &[signal(1, ExistenceResult::StillExists, 400, now)],
                now,
                &thresholds
            ),
            Confidence::Stale
        );
    }

    #[test]
    fn confidence_latest_positive_wins() {
        let now = tz();
        let thresholds = crate::freshness::DEFAULT_THRESHOLDS;
        // One stale user + one fresh user → fresh (recent) confirms.
        let signals = vec![
            signal(1, ExistenceResult::StillExists, 200, now),
            signal(2, ExistenceResult::StillExists, 0, now),
        ];
        assert_eq!(
            confidence(&signals, now, &thresholds),
            Confidence::RecentlyVerified
        );
    }

    #[test]
    fn confidence_no_longer_exists_is_conflicting_regardless_of_positives() {
        let now = tz();
        let thresholds = crate::freshness::DEFAULT_THRESHOLDS;
        let signals = vec![
            signal(1, ExistenceResult::StillExists, 0, now),
            signal(2, ExistenceResult::NoLongerExists, 1, now),
        ];
        assert_eq!(
            confidence(&signals, now, &thresholds),
            Confidence::Conflicting
        );
    }

    #[test]
    fn confidence_info_changed_only_is_reported_not_conflicting() {
        let now = tz();
        let thresholds = crate::freshness::DEFAULT_THRESHOLDS;
        let signals = vec![signal(1, ExistenceResult::InfoChanged, 0, now)];
        assert_eq!(confidence(&signals, now, &thresholds), Confidence::Reported);
    }
}
