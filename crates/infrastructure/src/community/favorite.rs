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
        .map_err(map_err)?;
        if exists.0 {
            sqlx::query("DELETE FROM favorite WHERE user_id = $1 AND location_id = $2")
                .bind(user.0)
                .bind(location_id)
                .execute(self.db.pool())
                .await
                .map_err(map_err)?;
            Ok(false)
        } else {
            sqlx::query(
                "INSERT INTO favorite (user_id, location_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(user.0)
            .bind(location_id)
            .execute(self.db.pool())
            .await
            .map_err(map_err)?;
            Ok(true)
        }
    }

    async fn is_favorited(&self, user: UserId, location_id: i64) -> Result<bool, ContributionError> {
        let row: (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM favorite WHERE user_id = $1 AND location_id = $2)",
        )
        .bind(user.0)
        .bind(location_id)
        .fetch_one(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(row.0)
    }

    async fn list(&self, user: UserId) -> Result<Vec<i64>, ContributionError> {
        struct Row {
            location_id: i64,
        }
        let rows = sqlx::query_as!(
            Row,
            r#"
            SELECT location_id
            FROM favorite
            WHERE user_id = $1
            ORDER BY created_at DESC, location_id DESC
            "#,
            user.0
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(rows.into_iter().map(|r| r.location_id).collect())
    }
}

fn map_err(_e: sqlx::Error) -> ContributionError {
    ContributionError::Internal
}
