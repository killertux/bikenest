use std::time::Duration;

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

fn env_u32(key: &str) -> Option<u32> {
    std::env::var(key).ok().and_then(|v| v.parse().ok())
}
