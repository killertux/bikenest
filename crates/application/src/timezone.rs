//! Timezone-resolution port (§29 / plans/m3-community.md §5.1).
//!
//! Contributors supply arbitrary coordinates, so a destination's IANA timezone
//! can no longer be a static map (as in M1). The infrastructure layer provides
//! an offline resolver; M7 may swap in a real geocoder reverse-timezone (Ledger
//! #16 — keep the offline resolver as the fallback).

use async_trait::async_trait;
use bikenest_domain::GeoPoint;

#[derive(Debug, thiserror::Error)]
pub enum TimezoneError {
    #[error("timezone could not be resolved")]
    Unavailable,
    #[error("timezone resolution failed: {0}")]
    Unexpected(String),
}

/// Port: resolve an IANA timezone for a coordinate.
#[async_trait]
pub trait TimezoneResolver: Send + Sync {
    async fn resolve(&self, point: GeoPoint) -> Result<chrono_tz::Tz, TimezoneError>;
}
