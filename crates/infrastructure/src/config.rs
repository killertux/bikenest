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
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing environment variable: {0}")]
    MissingEnv(&'static str),
}

impl Config {
    /// Loads configuration from the process environment (typically populated
    /// from `.env` in development).
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            database_url: require_env("DATABASE_URL")?,
            bind_addr: std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            probe_timeout: Duration::from_secs(2),
        })
    }
}

fn require_env(key: &'static str) -> Result<String, ConfigError> {
    std::env::var(key).map_err(|_| ConfigError::MissingEnv(key))
}
