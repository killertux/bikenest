use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// Real PostgreSQL connection + migration runner (SQLx, hand-written SQL — §9/§10).
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
    pub async fn connect(database_url: &str) -> Result<Self, DbError> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(std::time::Duration::from_secs(5))
            .connect(database_url)
            .await
            .map_err(DbError::Connect)?;
        Ok(Self { pool })
    }

    /// Runs embedded migrations (`migrations/` at the workspace root).
    /// Idempotent: applied migrations are recorded and skipped.
    pub async fn migrate(&self) -> Result<(), DbError> {
        sqlx::migrate!("../../migrations")
            .run(&self.pool)
            .await
            .map_err(DbError::Migrate)?;
        Ok(())
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
            .map_err(|_| ProbeFailure::DbError)?;
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
