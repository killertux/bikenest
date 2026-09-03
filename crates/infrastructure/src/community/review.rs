//! SQL-backed review repository (plans/m3-community.md §6).
//!
//! One active review per user per location (§38). `upsert_review` is a single
//! transaction: insert-or-update the row, append a `review_revision` (prior
//! values), and recompute the location rating aggregate from `ACTIVE` reviews.

use crate::Db;
use async_trait::async_trait;
use bikenest_application::{ContributionError, Review, ReviewRepository};
use bikenest_domain::{ReviewBody, StarRating, UserId};

pub struct SqlxReviewRepository {
    db: Db,
}

impl SqlxReviewRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ReviewRepository for SqlxReviewRepository {
    async fn upsert_review(
        &self,
        location_id: i64,
        author: UserId,
        rating: StarRating,
        body: &ReviewBody,
    ) -> Result<(), ContributionError> {
        let mut tx = self.db.pool().begin().await.map_err(map_err)?;

        let existing: Option<(i64, i16, String)> = sqlx::query_as(
            "SELECT id, rating, body FROM review WHERE location_id = $1 AND author_id = $2",
        )
        .bind(location_id)
        .bind(author.0)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_err)?;

        if let Some((review_id, old_rating, old_body)) = existing {
            // Preserve the prior version before overwriting (§38 history).
            sqlx::query(
                "INSERT INTO review_revision (review_id, rating, body) VALUES ($1, $2, $3)",
            )
            .bind(review_id)
            .bind(old_rating)
            .bind(old_body)
            .execute(&mut *tx)
            .await
            .map_err(map_err)?;
            sqlx::query(
                "UPDATE review SET rating = $1, body = $2, updated_at = now() WHERE id = $3",
            )
            .bind(rating.as_i16())
            .bind(body.as_str())
            .bind(review_id)
            .execute(&mut *tx)
            .await
            .map_err(map_err)?;
        } else {
            let row: (i64,) = sqlx::query_as(
                r#"
                INSERT INTO review (location_id, author_id, rating, body)
                VALUES ($1, $2, $3, $4)
                RETURNING id
                "#,
            )
            .bind(location_id)
            .bind(author.0)
            .bind(rating.as_i16())
            .bind(body.as_str())
            .fetch_one(&mut *tx)
            .await
            .map_err(map_err)?;
            let new_id = row.0;
            sqlx::query(
                "INSERT INTO review_revision (review_id, rating, body) VALUES ($1, $2, $3)",
            )
            .bind(new_id)
            .bind(rating.as_i16())
            .bind(body.as_str())
            .execute(&mut *tx)
            .await
            .map_err(map_err)?;
        }

        // Recompute the denormalized aggregate in the same transaction (no drift).
        sqlx::query(
            r#"
            UPDATE parking_location
            SET rating_avg = (
                    SELECT AVG(rating)::numeric(3,2) FROM review
                    WHERE location_id = $1 AND moderation_state = 'ACTIVE'
                ),
                rating_count = (
                    SELECT COUNT(*)::integer FROM review
                    WHERE location_id = $1 AND moderation_state = 'ACTIVE'
                )
            WHERE id = $1
            "#,
        )
        .bind(location_id)
        .execute(&mut *tx)
        .await
        .map_err(map_err)?;

        tx.commit().await.map_err(map_err)?;
        Ok(())
    }

    async fn find_own(
        &self,
        location_id: i64,
        author: UserId,
    ) -> Result<Option<Review>, ContributionError> {
        let row = sqlx::query_as::<_, ReviewRow>(r#"
            SELECT id, location_id, author_id, rating, body, created_at, updated_at
            FROM review
            WHERE location_id = $1 AND author_id = $2
            "#).bind(location_id).bind(author.0)
        .fetch_optional(self.db.pool())
        .await
        .map_err(map_err)?;
        row.map(review_from_row).transpose()
    }

    async fn list_active(&self, location_id: i64) -> Result<Vec<Review>, ContributionError> {
        let rows = sqlx::query_as::<_, ReviewRow>(r#"
            SELECT id, location_id, author_id, rating, body, created_at, updated_at
            FROM review
            WHERE location_id = $1 AND moderation_state = 'ACTIVE'
            ORDER BY created_at DESC, id DESC
            "#).bind(location_id)
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;
        rows.into_iter().map(review_from_row).collect()
    }
}

#[derive(sqlx::FromRow)]
struct ReviewRow {
    id: i64,
    location_id: i64,
    author_id: Option<i64>,
    rating: i16,
    body: String,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

fn review_from_row(r: ReviewRow) -> Result<Review, ContributionError> {
    Ok(Review {
        id: r.id,
        location_id: r.location_id,
        author: r.author_id.map(UserId),
        rating: StarRating::from_smallint(r.rating)
            .map_err(|e| ContributionError::InvalidField(e.to_string()))?,
        body: ReviewBody::new(&r.body)
            .map_err(|e| ContributionError::InvalidField(e.to_string()))?,
        created_at: r.created_at,
        updated_at: r.updated_at,
    })
}

fn map_err(_e: sqlx::Error) -> ContributionError {
    ContributionError::Internal
}
