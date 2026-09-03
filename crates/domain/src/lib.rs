//! BikeNest domain crate.
//!
//! Pure business concepts. MUST NOT depend on axum, sqlx, askama, or any
//! infrastructure (REQUIREMENTS §4 / §113).

use thiserror::Error;

pub mod auth;
pub mod community;
pub mod freshness;
pub mod hours;
pub mod moderation;
pub mod parking;
pub mod photo;
pub mod privacy;

pub use auth::{
    AccountState, AuthenticationProvider, CsrfToken, Password, PasswordPolicy, ProviderIdentity,
    Role, SessionId, User, VerificationToken,
};
pub use community::{
    AttributeResult, ChangeKind, Confidence, ExistenceResult, ExistenceSignal, ProposalKind,
    ProposalStatus, ReviewBody, RevisionSummary, StarRating, VerificationKind, confidence,
    is_known_attribute_code,
};
pub use freshness::{DEFAULT_THRESHOLDS, FreshnessCategory, FreshnessThresholds, categorize};
pub use hours::{OpenStatus, OpeningHours, TimeRange, hms};
pub use moderation::{
    ModerationLimits, REPORT_REASONS, ReportDescription, ReportOutcome, ReportState,
    ReportTargetType, is_known_report_reason, reason_allowed_for,
};
pub use parking::{
    Cost, CurrencyCode, GeoPoint, ModerationState, Money, ParkingLocation, ParkingType,
    PricingUnit, Rating, SECURITY_FEATURE_CODES, SecurityFeature, SecurityState,
    is_known_security_code,
};
pub use photo::{
    ALLOWED_INPUT_FORMATS, DERIVATIVE_QUALITY, MAX_PHOTO_BYTES, MAX_PHOTO_MEGAPIXELS,
    PhotoDimensions, PhotoLimits, PhotoModerationState, THUMBNAIL_MAX_SIDE, bytes_within_limit,
    format_allowed,
};
pub use privacy::{
    ExportState, PolicyKind, PrivacyRequestKind, PrivacyRequestState, RetentionPolicy,
    anonymized_email,
};

/// Database identifier of a user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserId(pub i64);

/// Validated, normalized email address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserEmail(String);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("email must not be empty")]
    EmptyEmail,
    #[error("email is not valid: {0}")]
    InvalidEmail(String),
    #[error("{0}")]
    Invalid(String),
    #[error("password does not meet the policy")]
    WeakPassword,
    #[error("unknown role code: {0}")]
    InvalidRole(String),
    #[error("unknown account state code: {0}")]
    InvalidState(String),
}

impl UserEmail {
    /// Validates and normalizes (lowercase, trimmed) an email address.
    ///
    /// M0 keeps validation deliberately simple (shape check only);
    /// full RFC-style validation is unnecessary for storage and
    /// deliverability is confirmed by the verification flow (M2).
    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        let normalized = raw.trim().to_lowercase();
        if normalized.is_empty() {
            return Err(DomainError::EmptyEmail);
        }
        let Some((local, domain)) = normalized.split_once('@') else {
            return Err(DomainError::InvalidEmail(raw.to_string()));
        };
        if local.is_empty() || domain.is_empty() || !domain.contains('.') {
            return Err(DomainError::InvalidEmail(raw.to_string()));
        }
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for UserEmail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_is_trimmed_and_lowercased() {
        let email = UserEmail::parse("  Ada@Example.COM ").expect("valid");
        assert_eq!(email.as_str(), "ada@example.com");
    }

    #[test]
    fn email_requires_local_part_domain_and_tld_dot() {
        assert_eq!(UserEmail::parse(""), Err(DomainError::EmptyEmail));
        assert!(UserEmail::parse("no-at-sign").is_err());
        assert!(UserEmail::parse("@example.com").is_err());
        assert!(UserEmail::parse("user@nodot").is_err());
        assert!(UserEmail::parse("user@example.com").is_ok());
    }
}
