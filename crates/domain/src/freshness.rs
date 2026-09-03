//! Information freshness (REQUIREMENTS §40).
//!
//! Thresholds are configurable product defaults, not claims about validity.

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreshnessThresholds {
    /// Fresh: verified within this many days.
    pub fresh_days: i64,
    /// Recently verified: fresh_days..recent_days.
    pub recent_days: i64,
    /// Aging: recent_days..aging_days.
    pub aging_days: i64,
    /// Stale: aging_days..stale_days. Beyond stale_days → VeryStale.
    pub stale_days: i64,
}

/// §40 recommended defaults.
pub const DEFAULT_THRESHOLDS: FreshnessThresholds = FreshnessThresholds {
    fresh_days: 30,
    recent_days: 90,
    aging_days: 180,
    stale_days: 365,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessCategory {
    Fresh,
    RecentlyVerified,
    Aging,
    Stale,
    VeryStale,
    /// Never verified.
    Never,
}

impl FreshnessCategory {
    pub fn as_code(&self) -> &'static str {
        match self {
            FreshnessCategory::Fresh => "fresh",
            FreshnessCategory::RecentlyVerified => "recently_verified",
            FreshnessCategory::Aging => "aging",
            FreshnessCategory::Stale => "stale",
            FreshnessCategory::VeryStale => "very_stale",
            FreshnessCategory::Never => "never",
        }
    }
}

/// Classify the freshness of information last verified at `last_verified_at`.
pub fn categorize(
    last_verified_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    thresholds: &FreshnessThresholds,
) -> FreshnessCategory {
    let Some(verified) = last_verified_at else {
        return FreshnessCategory::Never;
    };
    let days = (now - verified).num_days().max(0);
    if days < thresholds.fresh_days {
        FreshnessCategory::Fresh
    } else if days < thresholds.recent_days {
        FreshnessCategory::RecentlyVerified
    } else if days < thresholds.aging_days {
        FreshnessCategory::Aging
    } else if days < thresholds.stale_days {
        FreshnessCategory::Stale
    } else {
        FreshnessCategory::VeryStale
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn boundaries_match_documented_defaults() {
        let t = DEFAULT_THRESHOLDS;
        let now = Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap();
        let cases = [
            (29, FreshnessCategory::Fresh),
            (30, FreshnessCategory::RecentlyVerified),
            (89, FreshnessCategory::RecentlyVerified),
            (90, FreshnessCategory::Aging),
            (179, FreshnessCategory::Aging),
            (180, FreshnessCategory::Stale),
            (364, FreshnessCategory::Stale),
            (365, FreshnessCategory::VeryStale),
        ];
        for (days, expected) in cases {
            let verified = now - chrono::Duration::days(days);
            assert_eq!(categorize(Some(verified), now, &t), expected, "{days} days");
        }
    }

    #[test]
    fn never_verified_is_its_own_category() {
        let now = Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap();
        assert_eq!(
            categorize(None, now, &DEFAULT_THRESHOLDS),
            FreshnessCategory::Never
        );
    }
}
