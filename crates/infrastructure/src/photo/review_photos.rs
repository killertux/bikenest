//! SQL-backed reader for a review's approved photos (D3 §38). Only `APPROVED`
//! review photos render on the review card; hidden/rejected/pending are excluded.

use crate::Db;
use crate::parking::search::reader_err;
use async_trait::async_trait;
use bikenest_application::{ReaderError, ReviewPhotosReader, StoredPhoto};

pub struct SqlxReviewPhotosReader {
    db: Db,
}

impl SqlxReviewPhotosReader {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    storage_key: String,
    thumbnail_key: Option<String>,
}

#[async_trait]
impl ReviewPhotosReader for SqlxReviewPhotosReader {
    async fn photos(&self, review_id: i64) -> Result<Vec<StoredPhoto>, ReaderError> {
        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT storage_key, thumbnail_key
            FROM review_photo
            WHERE review_id = $1 AND moderation_state = 'APPROVED'
            ORDER BY position, id
            "#,
        )
        .bind(review_id)
        .fetch_all(self.db.pool())
        .await
        .map_err(|e| reader_err("review_photos.photos", e))?;

        Ok(rows
            .into_iter()
            .map(|r| StoredPhoto {
                key: r.storage_key,
                thumbnail_key: r.thumbnail_key,
                content_type: "image/jpeg".to_string(),
                alt: None,
            })
            .collect())
    }
}
