use std::time::Duration;

use bikenest_application::{FreshnessConfig, RecommendationConfig, DEFAULT_RECOMMENDATION_CONFIG};
use bikenest_domain::RetentionPolicy;

/// Environment-driven configuration (M0 surface is intentionally tiny).
#[derive(Debug, Clone)]
pub struct Config {
    /// PostgreSQL connection string, e.g. `postgres://bikenest:bikenest@localhost:5432/bikenest`.
    pub database_url: String,
    /// Socket address to bind, e.g. `0.0.0.0:8080`.
    pub bind_addr: String,
    /// Timeout applied to readiness probes.
    pub probe_timeout: Duration,
    /// Total retention for an exported personal-data payload (hours). §75.
    pub export_ttl_hours: u32,
    /// After this many inactive days an account is anonymized (0 = disabled,
    /// config-gated). §75.
    pub inactive_account_anonymize_after_days: u32,
    /// After this many days an anonymized account shell is purged (0 = disabled,
    /// config-gated). §75.
    pub deleted_account_purge_after_days: u32,
    /// Recommendation weights (§34, Ledger #8).
    pub recommendation: RecommendationConfig,
    /// Freshness/confidence thresholds (§40, Ledger #9/#17).
    pub freshness: FreshnessConfig,
    /// Privacy retention TTLs (§75, Ledger #20).
    pub retention: RetentionPolicy,
    /// Photo pipeline limits (§30/#44, Ledger #18).
    pub photo: PhotoConfig,
    /// Moderation limits (§43, Ledger #19).
    pub moderation: ModerationConfig,
}

/// Photo upload/derivative limits (§30/#44). Current domain constants are the
/// defaults; the domain crate keeps them as the enforcement values (see the
/// M7/M8 note — making runtime-plumbed limits is a follow-up).
#[derive(Debug, Clone, Copy)]
pub struct PhotoConfig {
    pub max_bytes: usize,
    pub max_megapixels: u64,
    pub thumb_max_side: u32,
    pub derivative_quality: u8,
}

impl Default for PhotoConfig {
    fn default() -> Self {
        Self {
            max_bytes: bikenest_domain::MAX_PHOTO_BYTES,
            max_megapixels: bikenest_domain::MAX_PHOTO_MEGAPIXELS,
            thumb_max_side: bikenest_domain::THUMBNAIL_MAX_SIDE,
            derivative_quality: bikenest_domain::DERIVATIVE_QUALITY,
        }
    }
}

/// Moderation limits (§43). Current constants are the defaults.
#[derive(Debug, Clone, Copy)]
pub struct ModerationConfig {
    pub report_description_max_len: usize,
    pub report_create_user_limit: u32,
    pub report_create_ip_limit: u32,
}

impl Default for ModerationConfig {
    fn default() -> Self {
        Self {
            report_description_max_len: 1000,
            report_create_user_limit: 10,
            report_create_ip_limit: 20,
        }
    }
}

impl Config {
    /// Loads configuration from the process environment (typically populated
    /// from `.env` in development).
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            database_url: require_env("DATABASE_URL")?,
            bind_addr: std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            probe_timeout: Duration::from_secs(2),
            export_ttl_hours: env_u32("EXPORT_TTL_HOURS").unwrap_or(24),
            inactive_account_anonymize_after_days: env_u32("INACTIVE_ACCOUNT_ANONYMIZE_AFTER_DAYS")
                .unwrap_or(0),
            deleted_account_purge_after_days: env_u32("DELETED_ACCOUNT_PURGE_AFTER_DAYS")
                .unwrap_or(0),
            recommendation: recommendation_config_from_env(),
            freshness: freshness_config_from_env(),
            retention: retention_from_env(),
            photo: photo_config_from_env(),
            moderation: ModerationConfig {
                report_description_max_len: env_usize("MOD_REPORT_DESC_MAX_LEN").unwrap_or(1000),
                report_create_user_limit: env_u32("MOD_REPORT_USER_LIMIT").unwrap_or(10),
                report_create_ip_limit: env_u32("MOD_REPORT_IP_LIMIT").unwrap_or(20),
            },
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing environment variable: {0}")]
    MissingEnv(&'static str),
}

fn require_env(key: &'static str) -> Result<String, ConfigError> {
    std::env::var(key).map_err(|_| ConfigError::MissingEnv(key))
}

/// Recommendation weights (§34, Ledger #8), from env with M1 defaults.
pub fn recommendation_config_from_env() -> RecommendationConfig {
    let rec = DEFAULT_RECOMMENDATION_CONFIG;
    RecommendationConfig {
        w_distance: env_f64("REC_WEIGHT_DISTANCE").unwrap_or(rec.w_distance),
        w_security: env_f64("REC_WEIGHT_SECURITY").unwrap_or(rec.w_security),
        w_rating: env_f64("REC_WEIGHT_RATING").unwrap_or(rec.w_rating),
        w_freshness: env_f64("REC_WEIGHT_FRESHNESS").unwrap_or(rec.w_freshness),
        w_verification: env_f64("REC_WEIGHT_VERIFICATION").unwrap_or(rec.w_verification),
        candidate_cap: env_usize("REC_CANDIDATE_CAP").unwrap_or(rec.candidate_cap),
    }
}

/// Freshness thresholds (§40, Ledger #9/#17), from env with M1 defaults.
pub fn freshness_config_from_env() -> FreshnessConfig {
    FreshnessConfig {
        thresholds: bikenest_domain::FreshnessThresholds {
            fresh_days: env_i64("FRESHNESS_FRESH_DAYS")
                .unwrap_or(bikenest_domain::DEFAULT_THRESHOLDS.fresh_days),
            recent_days: env_i64("FRESHNESS_RECENT_DAYS")
                .unwrap_or(bikenest_domain::DEFAULT_THRESHOLDS.recent_days),
            aging_days: env_i64("FRESHNESS_AGING_DAYS")
                .unwrap_or(bikenest_domain::DEFAULT_THRESHOLDS.aging_days),
            stale_days: env_i64("FRESHNESS_STALE_DAYS")
                .unwrap_or(bikenest_domain::DEFAULT_THRESHOLDS.stale_days),
        },
    }
}

/// Photo pipeline limits (§30/#44, Ledger #18), from env with M4 defaults.
pub fn photo_config_from_env() -> PhotoConfig {
    PhotoConfig {
        max_bytes: env_usize("PHOTO_MAX_BYTES").unwrap_or(bikenest_domain::MAX_PHOTO_BYTES),
        max_megapixels: env_u64("PHOTO_MAX_MEGAPIXELS").unwrap_or(bikenest_domain::MAX_PHOTO_MEGAPIXELS),
        thumb_max_side: env_u32("PHOTO_THUMB_MAX_SIDE").unwrap_or(bikenest_domain::THUMBNAIL_MAX_SIDE),
        derivative_quality: env_u8("PHOTO_DERIVATIVE_QUALITY").unwrap_or(bikenest_domain::DERIVATIVE_QUALITY),
    }
}

fn env_u32(key: &str) -> Option<u32> {
    std::env::var(key).ok().and_then(|v| v.parse().ok())
}

fn env_u64(key: &str) -> Option<u64> {
    std::env::var(key).ok().and_then(|v| v.parse().ok())
}

fn env_usize(key: &str) -> Option<usize> {
    std::env::var(key).ok().and_then(|v| v.parse().ok())
}

fn env_u8(key: &str) -> Option<u8> {
    std::env::var(key).ok().and_then(|v| v.parse().ok())
}

fn env_i64(key: &str) -> Option<i64> {
    std::env::var(key).ok().and_then(|v| v.parse().ok())
}

fn env_f64(key: &str) -> Option<f64> {
    std::env::var(key).ok().and_then(|v| v.parse().ok())
}

/// Retention TTLs (§75, Ledger #20). Values are seconds; each defaults to the
/// `RetentionPolicy::default()` value when unset.
fn retention_from_env() -> RetentionPolicy {
    let d = RetentionPolicy::default();
    RetentionPolicy {
        password_reset_ttl: chrono::Duration::seconds(
            env_i64("RETENTION_PASSWORD_RESET_SECONDS").unwrap_or(d.password_reset_ttl.num_seconds()),
        ),
        email_verification_ttl: chrono::Duration::seconds(
            env_i64("RETENTION_EMAIL_VERIFICATION_SECONDS").unwrap_or(d.email_verification_ttl.num_seconds()),
        ),
        session_idle: chrono::Duration::seconds(
            env_i64("RETENTION_SESSION_IDLE_SECONDS").unwrap_or(d.session_idle.num_seconds()),
        ),
        parked_here_ttl: chrono::Duration::seconds(
            env_i64("RETENTION_PARKED_HERE_SECONDS").unwrap_or(d.parked_here_ttl.num_seconds()),
        ),
        export_ttl: chrono::Duration::seconds(
            env_i64("RETENTION_EXPORT_SECONDS").unwrap_or(d.export_ttl.num_seconds()),
        ),
        upload_orphan_ttl: chrono::Duration::seconds(
            env_i64("RETENTION_UPLOAD_ORPHAN_SECONDS").unwrap_or(d.upload_orphan_ttl.num_seconds()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bikenest_application::DEFAULT_RECOMMENDATION_CONFIG;

    // With no env overrides set (the test environment), each *Config helper must
    // fall back to the documented M1/M4/M5/§75 defaults.
    #[test]
    fn recommendation_defaults_match_m1() {
        let got = recommendation_config_from_env();
        let exp = DEFAULT_RECOMMENDATION_CONFIG;
        assert_eq!(got.w_distance, exp.w_distance);
        assert_eq!(got.w_security, exp.w_security);
        assert_eq!(got.w_rating, exp.w_rating);
        assert_eq!(got.w_freshness, exp.w_freshness);
        assert_eq!(got.w_verification, exp.w_verification);
        assert_eq!(got.candidate_cap, exp.candidate_cap);
    }

    #[test]
    fn freshness_defaults_match_m1() {
        let got = freshness_config_from_env();
        assert_eq!(got.thresholds, bikenest_domain::DEFAULT_THRESHOLDS);
    }

    #[test]
    fn photo_defaults_match_m4() {
        let got = photo_config_from_env();
        assert_eq!(got.max_bytes, bikenest_domain::MAX_PHOTO_BYTES);
        assert_eq!(got.max_megapixels, bikenest_domain::MAX_PHOTO_MEGAPIXELS);
        assert_eq!(got.thumb_max_side, bikenest_domain::THUMBNAIL_MAX_SIDE);
        assert_eq!(got.derivative_quality, bikenest_domain::DERIVATIVE_QUALITY);
    }

    #[test]
    fn retention_defaults_match_policy() {
        assert_eq!(retention_from_env().session_idle, RetentionPolicy::default().session_idle);
        assert_eq!(retention_from_env().parked_here_ttl, RetentionPolicy::default().parked_here_ttl);
        assert_eq!(retention_from_env().export_ttl, RetentionPolicy::default().export_ttl);
    }

    #[test]
    fn moderation_defaults() {
        let m = ModerationConfig::default();
        assert_eq!(m.report_description_max_len, 1000);
        assert_eq!(m.report_create_user_limit, 10);
        assert_eq!(m.report_create_ip_limit, 20);
    }
}
