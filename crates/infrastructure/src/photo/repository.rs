//! SQL-backed photo repository (M4): queue insert, moderation flips and the
//! pending-photo read model.

use crate::Db;
use async_trait::async_trait;
use bikenest_application::{
    NewPendingPhoto, PendingPhoto, PhotoError, PhotoForModeration, PhotoRepository, RejectedPhoto,
};
use bikenest_domain::{PhotoDimensions, PhotoModerationState, UserId};

pub struct SqlxPhotoRepository {
    db: Db,
}

impl SqlxPhotoRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl PhotoRepository for SqlxPhotoRepository {
    async fn insert_pending(&self, p: &NewPendingPhoto) -> Result<i64, PhotoError> {
        struct IdRow {
            id: i64,
        }
        let row = sqlx::query_as!(
            IdRow,
            r#"
            INSERT INTO parking_photo
                (location_id, storage_key, content_type, alt, position, moderation_state, uploader_id)
            VALUES ($1, '', $2, $3, 0, 'PENDING_REVIEW', $4)
            RETURNING id
            "#,
            p.location_id,
            p.content_type,
            p.alt,
            p.uploader_id.0,
        )
        .fetch_one(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(row.id)
    }

    async fn max_position(&self, location_id: i64) -> Result<i32, PhotoError> {
        let row = sqlx::query!(
            r#"SELECT COALESCE(MAX(position), 0) AS position FROM parking_photo WHERE location_id = $1"#,
            location_id
        )
        .fetch_one(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(row.position.unwrap_or(0))
    }

    async fn mark_processed(
        &self,
        id: i64,
        storage_key: &str,
        thumbnail_key: &str,
        dimensions: PhotoDimensions,
        processed_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), PhotoError> {
        sqlx::query!(
            r#"
            UPDATE parking_photo
            SET storage_key = $2, thumbnail_key = $3, width = $4, height = $5, processed_at = $6
            WHERE id = $1
            "#,
            id,
            storage_key,
            thumbnail_key,
            dimensions.width as i32,
            dimensions.height as i32,
            processed_at,
        )
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(())
    }

    async fn delete(&self, id: i64) -> Result<(), PhotoError> {
        sqlx::query!("DELETE FROM parking_photo WHERE id = $1", id)
            .execute(self.db.pool())
            .await
            .map_err(map_err)?;
        Ok(())
    }

    async fn approve(&self, id: i64, moderator: UserId, position: i32) -> Result<(), PhotoError> {
        let rows = sqlx::query!(
            r#"
            UPDATE parking_photo
            SET moderation_state = 'APPROVED', position = $3, reviewed_by = $2, reviewed_at = now()
            WHERE id = $1 AND moderation_state = 'PENDING_REVIEW'
            "#,
            id,
            moderator.0,
            position,
        )
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        if rows.rows_affected() != 1 {
            return Err(PhotoError::NotPending);
        }
        Ok(())
    }

    async fn reject(
        &self,
        id: i64,
        moderator: UserId,
        reason: &str,
    ) -> Result<RejectedPhoto, PhotoError> {
        struct RejectedRow {
            storage_key: String,
            thumbnail_key: Option<String>,
        }
        let row = sqlx::query_as!(
            RejectedRow,
            r#"
            UPDATE parking_photo
            SET moderation_state = 'REJECTED', rejection_reason = $3, reviewed_by = $2, reviewed_at = now()
            WHERE id = $1 AND moderation_state = 'PENDING_REVIEW'
            RETURNING storage_key, thumbnail_key
            "#,
            id,
            moderator.0,
            reason,
        )
        .fetch_optional(self.db.pool())
        .await
        .map_err(map_err)?;
        let Some(row) = row else {
            return Err(PhotoError::NotPending);
        };
        Ok(RejectedPhoto {
            storage_key: row.storage_key,
            thumbnail_key: row.thumbnail_key,
        })
    }

    async fn list_pending(&self) -> Result<Vec<PendingPhoto>, PhotoError> {
        struct PendingRow {
            id: i64,
            location_id: i64,
            location_name: String,
            storage_key: String,
            thumbnail_key: Option<String>,
            alt: Option<String>,
            width: Option<i32>,
            height: Option<i32>,
            uploader_id: Option<i64>,
            created_at: chrono::DateTime<chrono::Utc>,
        }
        let rows = sqlx::query_as!(
            PendingRow,
            r#"
            SELECT p.id, p.location_id, l.name AS location_name, p.storage_key,
                   p.thumbnail_key, p.alt, p.width, p.height, p.uploader_id, p.created_at
            FROM parking_photo p
            JOIN parking_location l ON l.id = p.location_id
            WHERE p.moderation_state = 'PENDING_REVIEW'
            ORDER BY p.created_at, p.id
            "#,
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;

        Ok(rows
            .into_iter()
            .map(|r| PendingPhoto {
                id: r.id,
                location_id: r.location_id,
                location_name: r.location_name,
                storage_key: r.storage_key,
                thumbnail_key: r.thumbnail_key,
                alt: r.alt,
                width: r.width,
                height: r.height,
                uploader_id: r.uploader_id.map(UserId),
                created_at: r.created_at,
            })
            .collect())
    }

    async fn get_for_moderation(&self, id: i64) -> Result<Option<PhotoForModeration>, PhotoError> {
        struct PhotoRow {
            id: i64,
            location_id: i64,
            moderation_state: String,
            storage_key: String,
            thumbnail_key: Option<String>,
        }
        let row = sqlx::query_as!(
            PhotoRow,
            r#"
            SELECT id, location_id, moderation_state, storage_key, thumbnail_key
            FROM parking_photo
            WHERE id = $1
            "#,
            id,
        )
        .fetch_optional(self.db.pool())
        .await
        .map_err(map_err)?;

        Ok(row.map(|r| PhotoForModeration {
            id: r.id,
            location_id: r.location_id,
            state: PhotoModerationState::from_code(&r.moderation_state).unwrap_or(
                PhotoModerationState::PendingReview,
            ),
            storage_key: r.storage_key,
            thumbnail_key: r.thumbnail_key,
        }))
    }
}

fn map_err(e: sqlx::Error) -> PhotoError {
    match e {
        // FK violation on insert → the location does not exist (§24).
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23503") => PhotoError::NotFound,
        sqlx::Error::Io(_) | sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed => {
            PhotoError::Internal
        }
        _ => PhotoError::Internal,
    }
}
