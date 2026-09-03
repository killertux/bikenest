//! Parking persistence (REQUIREMENTS §24–§32).

use crate::Db;

pub mod details;
pub mod photos;
pub mod search;
pub mod seed;

pub use details::SqlxParkingDetailsReader;
pub use photos::SqlxParkingPhotoReader;
pub use search::SqlxParkingSearchReader;
pub use seed::seed_mock;

/// IDs of ACTIVE (public) parking locations, for the sitemap (§111).
pub async fn active_parking_ids(db: &Db) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT id FROM parking_location WHERE moderation_state = 'ACTIVE' ORDER BY id",
    )
    .fetch_all(db.pool())
    .await
}
