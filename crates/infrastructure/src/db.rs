use crate::config::DbConfig;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Connection, Executor};

/// Real PostgreSQL connection + migration runner (SQLx, hand-written SQL).
#[derive(Debug, Clone)]
pub struct Db {
    pool: PgPool,
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("failed to connect to the database")]
    Connect(#[source] sqlx::Error),
    #[error("failed to run migrations")]
    Migrate(#[source] sqlx::migrate::MigrateError),
}

impl Db {
    /// Connects with the default [`DbConfig`] (10 connections, 5 s statement
    /// timeout). Tests and the test-support fixture use this.
    pub async fn connect(database_url: &str) -> Result<Self, DbError> {
        Self::connect_with(database_url, &DbConfig::default()).await
    }

    /// Connects with an explicit pool configuration.
    ///
    /// Every pooled connection is opened with a `statement_timeout` and an
    /// `idle_in_transaction_session_timeout` so one runaway query (or a
    /// transaction abandoned by a crashed client) cannot hold a connection —
    /// and, past `max_connections` of those, the whole pool — indefinitely.
    /// Connections are also recycled (`max_lifetime` / `idle_timeout`) so a
    /// failed-over primary is picked up without a restart.
    pub async fn connect_with(database_url: &str, config: &DbConfig) -> Result<Self, DbError> {
        let statement_timeout_ms = config.statement_timeout.as_millis().max(1) as u64;
        let idle_in_tx_ms = config.idle_in_tx_timeout.as_millis().max(1) as u64;
        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .acquire_timeout(config.acquire_timeout)
            .max_lifetime(config.max_lifetime)
            .idle_timeout(config.idle_timeout)
            .after_connect(move |conn, _meta| {
                Box::pin(async move {
                    conn.execute(
                        format!(
                            "SET statement_timeout = {statement_timeout_ms}; \
                             SET idle_in_transaction_session_timeout = {idle_in_tx_ms}"
                        )
                        .as_str(),
                    )
                    .await?;
                    Ok(())
                })
            })
            .connect(database_url)
            .await
            .map_err(DbError::Connect)?;
        Ok(Self { pool })
    }

    /// Runs embedded migrations (`migrations/` at the workspace root).
    /// Idempotent: applied migrations are recorded and skipped.
    ///
    /// Migrations run on a connection *detached* from the pool with
    /// `statement_timeout` disabled: an index build on a cold database
    /// legitimately takes minutes, and that is not what the per-request timeout
    /// defends against. The connection is closed afterwards rather than
    /// returned, so its relaxed settings never leak into request handling.
    pub async fn migrate(&self) -> Result<(), DbError> {
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(DbError::Connect)?
            .detach();
        conn.execute("SET statement_timeout = 0; SET idle_in_transaction_session_timeout = 0")
            .await
            .map_err(DbError::Connect)?;
        let result = sqlx::migrate!("../../migrations")
            .run(&mut conn)
            .await
            .map_err(DbError::Migrate);
        let _ = conn.close().await;
        result
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Wraps an existing pool (used by the test suite to share the
    /// test-support fixture).
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Simple liveness probe: `SELECT 1` with a short timeout.
    pub async fn ping(&self, timeout: std::time::Duration) -> Result<(), ProbeFailure> {
        let query = sqlx::query("SELECT 1");
        tokio::time::timeout(timeout, query.execute(&self.pool))
            .await
            .map_err(|_| ProbeFailure::Timeout)?
            .map_err(|e| {
                crate::db_error::classify_and_log("db.ping", e);
                ProbeFailure::DbError
            })?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProbeFailure {
    #[error("probe timed out")]
    Timeout,
    #[error("database error")]
    DbError,
}
