//! SQL-backed favorite repository (plans/m3-community.md §6).
//!
//! Favorites are per-user, idempotent and private (§42).

use crate::Db;
use async_trait::async_trait;
use bikenest_application::{ContributionError, FavoriteRepository};
use bikenest_domain::UserId;

pub struct SqlxFavoriteRepository {
    db: Db,
}

impl SqlxFavoriteRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl FavoriteRepository for SqlxFavoriteRepository {
    async fn toggle(&self, user: UserId, location_id: i64) -> Result<bool, ContributionError> {
        let exists: (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM favorite WHERE user_id = $1 AND location_id = $2)",
        )
        .bind(user.0)
        .bind(location_id)
        .fetch_one(self.db.pool())
        .await
        .map_err(|e| db_err("favorite.toggle", e))?;
        if exists.0 {
            sqlx::query("DELETE FROM favorite WHERE user_id = $1 AND location_id = $2")
                .bind(user.0)
                .bind(location_id)
                .execute(self.db.pool())
                .await
                .map_err(|e| db_err("favorite.toggle", e))?;
            Ok(false)
        } else {
            sqlx::query(
                "INSERT INTO favorite (user_id, location_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(user.0)
            .bind(location_id)
            .execute(self.db.pool())
            .await
            .map_err(|e| db_err("favorite.toggle", e))?;
            Ok(true)
        }
    }

    async fn is_favorited(
        &self,
        user: UserId,
        location_id: i64,
    ) -> Result<bool, ContributionError> {
        let row: (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM favorite WHERE user_id = $1 AND location_id = $2)",
        )
        .bind(user.0)
        .bind(location_id)
        .fetch_one(self.db.pool())
        .await
        .map_err(|e| db_err("favorite.is_favorited", e))?;
        Ok(row.0)
    }

    async fn list(&self, user: UserId) -> Result<Vec<i64>, ContributionError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            location_id: i64,
        }
        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT location_id
            FROM favorite
            WHERE user_id = $1
            ORDER BY created_at DESC, location_id DESC
            "#,
        )
        .bind(user.0)
        .fetch_all(self.db.pool())
        .await
        .map_err(|e| db_err("favorite.list", e))?;
        Ok(rows.into_iter().map(|r| r.location_id).collect())
    }
}

/// Classify + log the sqlx error (SQLSTATE, constraint), then map it onto
/// the feature error. `context` names the operation, e.g. `"favorite.toggle"`.
fn db_err(context: &'static str, e: sqlx::Error) -> ContributionError {
    crate::db_error::classify_and_log(context, e).into()
}
