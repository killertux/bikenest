//! SQL-backed reader for a review's approved photos (D3 §38). Only `APPROVED`
//! review photos render on the review card; hidden/rejected/pending are excluded.

use crate::parking::search::map_db_err;
use crate::Db;
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

struct Row {
    storage_key: String,
    thumbnail_key: Option<String>,
}

#[async_trait]
impl ReviewPhotosReader for SqlxReviewPhotosReader {
    async fn photos(&self, review_id: i64) -> Result<Vec<StoredPhoto>, ReaderError> {
        let rows = sqlx::query_as!(
            Row,
            r#"
            SELECT storage_key, thumbnail_key
            FROM review_photo
            WHERE review_id = $1 AND moderation_state = 'APPROVED'
            ORDER BY position, id
            "#,
            review_id
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(map_db_err)?;

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
