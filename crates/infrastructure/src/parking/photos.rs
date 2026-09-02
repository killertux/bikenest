//! SQL-backed reader for a location's approved photos (P3 gallery / P2 card).

use crate::parking::search::map_db_err;
use crate::Db;
use async_trait::async_trait;
use bikenest_application::{ParkingPhotoReader, ReaderError, StoredPhoto};

pub struct SqlxParkingPhotoReader {
    db: Db,
}

impl SqlxParkingPhotoReader {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

struct PhotoRow {
    storage_key: String,
    content_type: String,
    alt: Option<String>,
}

#[async_trait]
impl ParkingPhotoReader for SqlxParkingPhotoReader {
    async fn photos(&self, location_id: i64) -> Result<Vec<StoredPhoto>, ReaderError> {
        let rows = sqlx::query_as!(
            PhotoRow,
            r#"
            SELECT storage_key, content_type, alt
            FROM parking_photo
            WHERE location_id = $1 AND moderation_state = 'APPROVED'
            ORDER BY position, id
            "#,
            location_id
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(map_db_err)?;

        Ok(rows
            .into_iter()
            .map(|r| StoredPhoto {
                key: r.storage_key,
                content_type: r.content_type,
                alt: r.alt,
            })
            .collect())
    }
}
