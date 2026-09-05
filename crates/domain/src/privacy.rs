//! Privacy and account-lifecycle domain values.
//!
//! Pure, no I/O. Models the privacy-request and export code lists, the
//! deterministic anonymized-email helper, and the retention TTL policy.
//! The application [`PrivacyService`](crate::application::privacy::PrivacyService)
//! and the infrastructure repositories are the consumers.

use crate::{DomainError, UserId};
use chrono::Duration;

// ---------------------------------------------------------------------------
// Privacy request kind
// ---------------------------------------------------------------------------

/// The seven data-subject request kinds. `Access` and `Export` are
/// fulfilled automatically by the export flow; `Deletion` by the account-
/// deletion flow; the remaining four are manual (recorded + operator-fulfilled).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyRequestKind {
    Access,
    Rectification,
    Deletion,
    Export,
    Restriction,
    Objection,
    ConsentWithdrawal,
}

impl PrivacyRequestKind {
    pub fn as_code(self) -> &'static str {
        match self {
            PrivacyRequestKind::Access => "access",
            PrivacyRequestKind::Rectification => "rectification",
            PrivacyRequestKind::Deletion => "deletion",
            PrivacyRequestKind::Export => "export",
            PrivacyRequestKind::Restriction => "restriction",
            PrivacyRequestKind::Objection => "objection",
            PrivacyRequestKind::ConsentWithdrawal => "consent_withdrawal",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, DomainError> {
        match code {
            "access" => Ok(PrivacyRequestKind::Access),
            "rectification" => Ok(PrivacyRequestKind::Rectification),
            "deletion" => Ok(PrivacyRequestKind::Deletion),
            "export" => Ok(PrivacyRequestKind::Export),
            "restriction" => Ok(PrivacyRequestKind::Restriction),
            "objection" => Ok(PrivacyRequestKind::Objection),
            "consent_withdrawal" => Ok(PrivacyRequestKind::ConsentWithdrawal),
            other => Err(DomainError::Invalid(format!(
                "unknown privacy request kind: {other}"
            ))),
        }
    }
}

/// How long the operator has to answer a data-subject request, in days.
///
/// 15 days is LGPD art. 19's deadline for a full reply, which is the stricter
/// of the two regimes this product serves (GDPR art. 12 allows one month) and
/// the figure `docs/legal-review.md` already commits to. There is no env knob
/// for it: a legal deadline is not an operator preference.
pub const PRIVACY_REQUEST_SLA_DAYS: i64 = 15;

/// Days left before a request filed at `created_at` breaches
/// [`PRIVACY_REQUEST_SLA_DAYS`]. Negative once the deadline has passed, so the
/// queue can show "3 days left" and "2 days overdue" from one number.
pub fn privacy_request_days_left(
    created_at: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> i64 {
    let due = created_at + Duration::days(PRIVACY_REQUEST_SLA_DAYS);
    (due - now).num_days()
}

// ---------------------------------------------------------------------------
// Privacy request state
// ---------------------------------------------------------------------------

/// Lifecycle of a manual rights request. Automatic flows (export/deletion)
/// create these as evidence but drive them straight to `Completed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyRequestState {
    Open,
    InProgress,
    Completed,
    Declined,
}

impl PrivacyRequestState {
    pub fn as_code(self) -> &'static str {
        match self {
            PrivacyRequestState::Open => "OPEN",
            PrivacyRequestState::InProgress => "IN_PROGRESS",
            PrivacyRequestState::Completed => "COMPLETED",
            PrivacyRequestState::Declined => "DECLINED",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, DomainError> {
        match code {
            "OPEN" => Ok(PrivacyRequestState::Open),
            "IN_PROGRESS" => Ok(PrivacyRequestState::InProgress),
            "COMPLETED" => Ok(PrivacyRequestState::Completed),
            "DECLINED" => Ok(PrivacyRequestState::Declined),
            other => Err(DomainError::Invalid(format!(
                "unknown privacy request state: {other}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Personal-data export state
// ---------------------------------------------------------------------------

/// Lifecycle of one personal-data export. `Ready` (payload assembled
/// synchronously), `Downloaded` (single-use link consumed), `Expired` (24h TTL).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportState {
    Ready,
    Downloaded,
    Expired,
}

impl ExportState {
    pub fn as_code(self) -> &'static str {
        match self {
            ExportState::Ready => "READY",
            ExportState::Downloaded => "DOWNLOADED",
            ExportState::Expired => "EXPIRED",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, DomainError> {
        match code {
            "READY" => Ok(ExportState::Ready),
            "DOWNLOADED" => Ok(ExportState::Downloaded),
            "EXPIRED" => Ok(ExportState::Expired),
            other => Err(DomainError::Invalid(format!(
                "unknown export state: {other}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Policy kind
// ---------------------------------------------------------------------------

/// The versioned legal-page kinds. These map one-to-one to `policy_version.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyKind {
    Privacy,
    Terms,
    Cookies,
}

impl PolicyKind {
    pub fn as_code(self) -> &'static str {
        match self {
            PolicyKind::Privacy => "privacy",
            PolicyKind::Terms => "terms",
            PolicyKind::Cookies => "cookies",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, DomainError> {
        match code {
            "privacy" => Ok(PolicyKind::Privacy),
            "terms" => Ok(PolicyKind::Terms),
            "cookies" => Ok(PolicyKind::Cookies),
            other => Err(DomainError::Invalid(format!(
                "unknown policy kind: {other}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Anonymized email
// ---------------------------------------------------------------------------

/// Deterministic, non-attributable, unique replacement email for an anonymized
/// account. `deleted+{id}@bikesnest.invalid`:
///
/// - **deterministic** (no randomness → resume-safe, idempotent),
/// - **unique per id** (preserves the `lower(email)` unique index),
/// - **non-attributable** (no part of the original address survives),
/// - **undeliverable** (`bikesnest.invalid` is RFC 2606 `invalid`-reserved).
pub fn anonymized_email(user_id: UserId) -> String {
    format!("deleted+{}@bikesnest.invalid", user_id.0)
}

// ---------------------------------------------------------------------------
// Retention TTLs
// ---------------------------------------------------------------------------

/// Retention TTL constants used by privacy and account-lifecycle flows.
///
/// Hardcoded now; configuration can be added at the application boundary. These are
/// the single source of truth for the issue-time TTLs (M2/M3) **and** the
/// retention purge thresholds.
#[derive(Debug, Clone, Copy)]
pub struct RetentionPolicy {
    /// Password-reset token lifetime.
    pub password_reset_ttl: Duration,
    /// Email-verification token lifetime.
    pub email_verification_ttl: Duration,
    /// Server-side session idle timeout (cookie Max-Age parity).
    pub session_idle: Duration,
    /// "I parked here" verification lifetime.
    pub parked_here_ttl: Duration,
    /// Temporary personal-data export lifetime.
    pub export_ttl: Duration,
    /// Orphaned upload-object sweep threshold.
    pub upload_orphan_ttl: Duration,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            password_reset_ttl: Duration::hours(1),
            email_verification_ttl: Duration::hours(24),
            session_idle: Duration::days(30),
            parked_here_ttl: Duration::days(90),
            export_ttl: Duration::hours(24),
            upload_orphan_ttl: Duration::hours(24),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_request_kind_codes_round_trip() {
        for kind in [
            PrivacyRequestKind::Access,
            PrivacyRequestKind::Rectification,
            PrivacyRequestKind::Deletion,
            PrivacyRequestKind::Export,
            PrivacyRequestKind::Restriction,
            PrivacyRequestKind::Objection,
            PrivacyRequestKind::ConsentWithdrawal,
        ] {
            assert_eq!(PrivacyRequestKind::from_code(kind.as_code()), Ok(kind));
        }
        assert!(PrivacyRequestKind::from_code("track").is_err());
    }

    #[test]
    fn privacy_request_state_codes_round_trip() {
        for state in [
            PrivacyRequestState::Open,
            PrivacyRequestState::InProgress,
            PrivacyRequestState::Completed,
            PrivacyRequestState::Declined,
        ] {
            assert_eq!(PrivacyRequestState::from_code(state.as_code()), Ok(state));
        }
        assert!(PrivacyRequestState::from_code("PENDING").is_err());
    }

    #[test]
    fn export_state_codes_round_trip() {
        for state in [
            ExportState::Ready,
            ExportState::Downloaded,
            ExportState::Expired,
        ] {
            assert_eq!(ExportState::from_code(state.as_code()), Ok(state));
        }
        assert!(ExportState::from_code("PROCESSING").is_err());
    }

    #[test]
    fn policy_kind_codes_round_trip() {
        for kind in [PolicyKind::Privacy, PolicyKind::Terms, PolicyKind::Cookies] {
            assert_eq!(PolicyKind::from_code(kind.as_code()), Ok(kind));
        }
        assert!(PolicyKind::from_code("about").is_err());
    }

    #[test]
    fn anonymized_email_is_deterministic_and_unique() {
        let a = anonymized_email(UserId(42));
        let b = anonymized_email(UserId(42));
        assert_eq!(a, b);
        // Unique across distinct ids.
        assert_ne!(a, anonymized_email(UserId(43)));
        // RFC 2606 `.invalid` so it can never receive mail.
        assert!(a.ends_with("@bikesnest.invalid"));
    }

    #[test]
    fn anonymized_email_does_not_contain_input() {
        let email = anonymized_email(UserId(7));
        assert_eq!(email, "deleted+7@bikesnest.invalid");
        assert!(!email.contains("deleted+7@bikesnest.invalid@"));
    }

    #[test]
    fn retention_policy_defaults_match_plan() {
        let p = RetentionPolicy::default();
        assert_eq!(p.password_reset_ttl, Duration::hours(1));
        assert_eq!(p.email_verification_ttl, Duration::hours(24));
        assert_eq!(p.session_idle, Duration::days(30));
        assert_eq!(p.parked_here_ttl, Duration::days(90));
        assert_eq!(p.export_ttl, Duration::hours(24));
        assert_eq!(p.upload_orphan_ttl, Duration::hours(24));
    }
}
