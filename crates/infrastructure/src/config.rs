use std::time::Duration;

use bikenest_application::{DEFAULT_RECOMMENDATION_CONFIG, FreshnessConfig, RecommendationConfig};
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
    /// Background job queue (plans/m9-background-jobs.md).
    pub jobs: JobConfig,
}

/// Photo upload/derivative limits (§30/#44, Ledger #18), env-driven with the
/// domain constants as defaults. Runtime-plumbed through the photo pipeline in
/// M8 so operators can tune without a rebuild.
pub type PhotoConfig = bikenest_domain::PhotoLimits;

/// Moderation limits (§43, Ledger #19), env-driven with the domain constants as
/// defaults. Runtime-plumbed through the moderation service in M8.
pub type ModerationConfig = bikenest_domain::ModerationLimits;

/// Background job queue knobs (plans/m9-background-jobs.md). Defaults target
/// a single-instance dev worker; tune per deployment.
#[derive(Debug, Clone, Copy)]
pub struct JobConfig {
    /// Spawn the in-process worker loop at startup (set false for web-only instances).
    pub enabled: bool,
    /// How often the worker polls the queue when idle.
    pub poll_interval: Duration,
    /// How many jobs a worker claims per batch.
    pub batch_size: usize,
    /// Lease length; a running job heartbeats to hold it, and a crashed worker's
    /// lease expires so another worker can re-claim.
    pub lease_ttl: Duration,
    /// Retry budget per job (overridable per row at enqueue).
    pub max_attempts: i32,
    /// Exponential backoff base; actual delay = base * 2^(attempt-1) + jitter.
    pub backoff_base_ms: u64,
    /// Terminal rows older than this many days are deleted by `jobs.gc`.
    pub history_retention_days: u32,
}

impl Default for JobConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            poll_interval: Duration::from_secs(5),
            batch_size: 4,
            lease_ttl: Duration::from_secs(600),
            max_attempts: 5,
            backoff_base_ms: 2000,
            history_retention_days: 7,
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
            moderation: moderation_config_from_env(),
            jobs: job_config_from_env(),
        })
    }
}

/// Job queue knobs (plans/m9-background-jobs.md), from env with sane defaults.
pub fn job_config_from_env() -> JobConfig {
    JobConfig {
        enabled: env_bool("JOBS_ENABLED").unwrap_or(true),
        poll_interval: Duration::from_millis(env_u64("JOBS_POLL_INTERVAL_MS").unwrap_or(5000)),
        batch_size: env_usize("JOBS_BATCH_SIZE").unwrap_or(4),
        lease_ttl: Duration::from_millis(env_u64("JOBS_LEASE_TTL_MS").unwrap_or(600_000)),
        max_attempts: env_i64("JOBS_MAX_ATTEMPTS").unwrap_or(5) as i32,
        backoff_base_ms: env_u64("JOBS_BACKOFF_BASE_MS").unwrap_or(2000),
        history_retention_days: env_u32("JOBS_HISTORY_RETENTION_DAYS").unwrap_or(7),
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
        max_megapixels: env_u64("PHOTO_MAX_MEGAPIXELS")
            .unwrap_or(bikenest_domain::MAX_PHOTO_MEGAPIXELS),
        thumb_max_side: env_u32("PHOTO_THUMB_MAX_SIDE")
            .unwrap_or(bikenest_domain::THUMBNAIL_MAX_SIDE),
        derivative_quality: env_u8("PHOTO_DERIVATIVE_QUALITY")
            .unwrap_or(bikenest_domain::DERIVATIVE_QUALITY),
    }
}

/// Moderation limits (§43, Ledger #19), from env with the domain defaults.
pub fn moderation_config_from_env() -> ModerationConfig {
    let d = bikenest_domain::ModerationLimits::default();
    ModerationConfig {
        report_description_max_len: env_usize("MOD_REPORT_DESC_MAX_LEN")
            .unwrap_or(d.report_description_max_len),
        report_create_user_limit: env_u32("MOD_REPORT_USER_LIMIT")
            .unwrap_or(d.report_create_user_limit),
        report_create_ip_limit: env_u32("MOD_REPORT_IP_LIMIT").unwrap_or(d.report_create_ip_limit),
    }
}

/// Client-side map configuration (**Ledger #3**). The style URL defaults to
/// MapLibre's public demo tiles; a Mapbox style also needs its public access
/// token embedded client-side (only when the style is Mapbox-based).
pub const DEFAULT_MAP_STYLE_URL: &str = "https://demotiles.maplibre.org/style.json";

#[derive(Debug, Clone)]
pub struct MapConfig {
    pub style_url: String,
    /// Public Mapbox token for the style/tiles; empty for a non-Mapbox style.
    pub access_token: String,
}

/// Is the style URL a Mapbox style (which needs a client-side access token)?
fn is_mapbox_style(style_url: &str) -> bool {
    style_url.starts_with("mapbox://") || style_url.contains("api.mapbox.com")
}

/// Pure resolver (separated from env so it is unit-testable without mutating
/// process global environment).
fn resolve_map_config(
    style_url: Option<String>,
    map_token: Option<String>,
    fallback_token: Option<String>,
) -> MapConfig {
    let style_url = style_url.unwrap_or_else(|| DEFAULT_MAP_STYLE_URL.to_string());
    let access_token = if is_mapbox_style(&style_url) {
        map_token.or(fallback_token).unwrap_or_default()
    } else {
        // Non-Mapbox style (e.g. demo tiles) needs no token; keep it off the page.
        String::new()
    };
    MapConfig {
        style_url,
        access_token,
    }
}

/// Build the map config from env: `MAP_STYLE_URL` (default demo tiles), plus the
/// Mapbox access token (`MAPBOX_MAP_ACCESS_TOKEN`, or the geocoder's
/// `MAPBOX_ACCESS_TOKEN` as a fallback) when the style is Mapbox-based.
pub fn map_config_from_env() -> MapConfig {
    resolve_map_config(
        std::env::var("MAP_STYLE_URL").ok(),
        std::env::var("MAPBOX_MAP_ACCESS_TOKEN").ok(),
        std::env::var("MAPBOX_ACCESS_TOKEN").ok(),
    )
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

fn env_bool(key: &str) -> Option<bool> {
    std::env::var(key)
        .ok()
        .and_then(|v| match v.trim().to_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
}

/// Retention TTLs (§75, Ledger #20). Values are seconds; each defaults to the
/// `RetentionPolicy::default()` value when unset.
fn retention_from_env() -> RetentionPolicy {
    let d = RetentionPolicy::default();
    RetentionPolicy {
        password_reset_ttl: chrono::Duration::seconds(
            env_i64("RETENTION_PASSWORD_RESET_SECONDS")
                .unwrap_or(d.password_reset_ttl.num_seconds()),
        ),
        email_verification_ttl: chrono::Duration::seconds(
            env_i64("RETENTION_EMAIL_VERIFICATION_SECONDS")
                .unwrap_or(d.email_verification_ttl.num_seconds()),
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
        assert_eq!(
            retention_from_env().session_idle,
            RetentionPolicy::default().session_idle
        );
        assert_eq!(
            retention_from_env().parked_here_ttl,
            RetentionPolicy::default().parked_here_ttl
        );
        assert_eq!(
            retention_from_env().export_ttl,
            RetentionPolicy::default().export_ttl
        );
    }

    #[test]
    fn moderation_defaults() {
        let m = ModerationConfig::default();
        assert_eq!(m.report_description_max_len, 1000);
        assert_eq!(m.report_create_user_limit, 10);
        assert_eq!(m.report_create_ip_limit, 20);
    }

    #[test]
    fn map_style_defaults_to_demo_tiles() {
        let c = resolve_map_config(None, None, None);
        assert_eq!(c.style_url, DEFAULT_MAP_STYLE_URL);
        assert_eq!(c.access_token, "");
    }

    #[test]
    fn mapbox_style_pulls_token() {
        let c = resolve_map_config(
            Some("mapbox://styles/u/s".to_string()),
            Some("public-map-tok".to_string()),
            Some("geo-tok".to_string()),
        );
        // The dedicated map token wins; the geocoder token is only a fallback.
        assert_eq!(c.access_token, "public-map-tok");

        let fallback = resolve_map_config(
            Some("https://api.mapbox.com/styles/v1/u/s".to_string()),
            None,
            Some("geo-tok".to_string()),
        );
        assert_eq!(fallback.access_token, "geo-tok");
    }

    #[test]
    fn non_mapbox_style_never_leaks_token() {
        // A non-Mapbox style (e.g. demo tiles / a self-hosted style) must not
        // embed a Mapbox token even when one is configured for geocoding.
        let c = resolve_map_config(
            Some("https://tiles.example/style.json".to_string()),
            None,
            Some("geo-tok".to_string()),
        );
        assert_eq!(c.access_token, "");
        assert!(!is_mapbox_style("https://tiles.example/style.json"));
        assert!(is_mapbox_style("mapbox://styles/u/s"));
        assert!(is_mapbox_style("https://api.mapbox.com/styles/v1/u/s"));
    }
}
