//! BikeNest domain crate.
//!
//! Pure business concepts. MUST NOT depend on axum, sqlx, askama, or any
//! infrastructure (REQUIREMENTS §4 / §113).

use thiserror::Error;

pub mod freshness;
pub mod hours;
pub mod parking;

pub use freshness::{categorize, FreshnessCategory, FreshnessThresholds, DEFAULT_THRESHOLDS};
pub use hours::{OpeningHours, OpenStatus, TimeRange, hms};
pub use parking::{
    is_known_security_code, Cost, CurrencyCode, GeoPoint, ModerationState, Money, ParkingLocation,
    ParkingType, PricingUnit, Rating, SecurityFeature, SecurityState, SECURITY_FEATURE_CODES,
};

/// A registered user of the application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: UserId,
    pub email: UserEmail,
    pub display_name: Option<String>,
}

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

impl User {
    pub fn new(id: UserId, email: UserEmail, display_name: Option<String>) -> Self {
        Self {
            id,
            email,
            display_name,
        }
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
