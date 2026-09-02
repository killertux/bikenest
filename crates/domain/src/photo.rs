//! Photo domain rules (REQUIREMENTS §30, §44–§45, §80).
//!
//! Pure constraints and the moderation state machine for the upload →
//! validate → process → moderate → publish pipeline. No I/O. The web and
//! application layers rely on these constants as the single source of truth
//! for validation and error messages.

use crate::DomainError;

/// Hard max upload size in bytes (§30): 10 MiB.
pub const MAX_PHOTO_BYTES: usize = 10 * 1024 * 1024;

/// Hard max decoded pixels (§30): 20 megapixels.
pub const MAX_PHOTO_MEGAPIXELS: u64 = 20;

/// Longest side of the JPEG thumbnail derivative (§80 derivative policy).
pub const THUMBNAIL_MAX_SIDE: u32 = 400;

/// Content-sniffed input formats (§30 allowlist). Sniffed by magic bytes,
/// never trusted from the filename extension.
pub const ALLOWED_INPUT_FORMATS: &[&str] = &["jpeg", "png", "webp"];

/// JPEG quality for the re-encoded full derivative.
pub const DERIVATIVE_QUALITY: u8 = 85;

/// Pixel dimensions of a processed derivative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhotoDimensions {
    pub width: u32,
    pub height: u32,
}

impl PhotoDimensions {
    /// Total pixel count, computed in u64 (u32*u32 cannot overflow u64).
    pub fn pixel_count(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Megapixels, rounded up (a 20.0 MP image is at the limit).
    pub fn megapixels(&self) -> u64 {
        self.pixel_count().div_ceil(1_000_000)
    }

    /// Whether the dimensions satisfy the §30 decoded-pixel cap.
    pub fn within_limit(&self) -> bool {
        self.megapixels() <= MAX_PHOTO_MEGAPIXELS
    }
}

/// Moderation lifecycle of a photo (§30/§116.2). Distinct from the location
/// [`ModerationState`](crate::parking::ModerationState) in the domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhotoModerationState {
    PendingReview,
    Approved,
    Rejected,
}

impl PhotoModerationState {
    pub fn as_code(self) -> &'static str {
        match self {
            PhotoModerationState::PendingReview => "PENDING_REVIEW",
            PhotoModerationState::Approved => "APPROVED",
            PhotoModerationState::Rejected => "REJECTED",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, DomainError> {
        match code {
            "PENDING_REVIEW" => Ok(PhotoModerationState::PendingReview),
            "APPROVED" => Ok(PhotoModerationState::Approved),
            "REJECTED" => Ok(PhotoModerationState::Rejected),
            other => Err(DomainError::Invalid(format!(
                "unknown photo moderation state: {other}"
            ))),
        }
    }

    /// Only approved photos are publicly visible (§30).
    pub fn is_publicly_visible(self) -> bool {
        matches!(self, PhotoModerationState::Approved)
    }
}

/// Whether raw byte length is within the §30 max upload size.
pub fn bytes_within_limit(bytes: usize) -> bool {
    bytes <= MAX_PHOTO_BYTES
}

/// Whether a content-sniffed format is in the §30 allowlist.
pub fn format_allowed(format: &str) -> bool {
    ALLOWED_INPUT_FORMATS.contains(&format)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moderation_state_codes_round_trip() {
        for s in [
            PhotoModerationState::PendingReview,
            PhotoModerationState::Approved,
            PhotoModerationState::Rejected,
        ] {
            assert_eq!(PhotoModerationState::from_code(s.as_code()), Ok(s));
        }
        assert!(PhotoModerationState::from_code("FLAGGED").is_err());
        assert!(PhotoModerationState::from_code("active").is_err());
    }

    #[test]
    fn only_approved_is_public() {
        assert!(!PhotoModerationState::PendingReview.is_publicly_visible());
        assert!(PhotoModerationState::Approved.is_publicly_visible());
        assert!(!PhotoModerationState::Rejected.is_publicly_visible());
    }

    #[test]
    fn byte_limit_boundary() {
        assert!(bytes_within_limit(MAX_PHOTO_BYTES));
        assert!(bytes_within_limit(MAX_PHOTO_BYTES - 1));
        assert!(!bytes_within_limit(MAX_PHOTO_BYTES + 1));
    }

    #[test]
    fn megapixel_limit_boundary_is_inclusive() {
        // Exactly 20 MP (20,000,000 px) = at the limit → allowed.
        assert!(PhotoDimensions { width: 5000, height: 4000 }.within_limit());
        // One pixel past the cap (20,000,001 px) rounds to 21 MP → over.
        assert!(!PhotoDimensions { width: 5000, height: 4001 }.within_limit());
        // Below the cap → allowed (a 19.995 MP image rounds up to 20 MP, still ≤ 20).
        assert!(PhotoDimensions { width: 5000, height: 3999 }.within_limit());
        // A small square is trivially fine.
        assert!(PhotoDimensions { width: 800, height: 600 }.within_limit());
    }

    #[test]
    fn megapixels_rounds_up() {
        assert_eq!(PhotoDimensions { width: 1000, height: 1000 }.megapixels(), 1);
        assert_eq!(PhotoDimensions { width: 1, height: 1 }.megapixels(), 1);
    }

    #[test]
    fn format_allowlist() {
        for f in ALLOWED_INPUT_FORMATS {
            assert!(format_allowed(f), "{f}");
        }
        assert!(!format_allowed("gif"));
        assert!(!format_allowed("tiff"));
        assert!(!format_allowed(""));
    }
}
