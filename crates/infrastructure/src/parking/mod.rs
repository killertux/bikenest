//! Parking persistence (REQUIREMENTS §24–§32).

pub mod details;
pub mod photos;
pub mod search;
pub mod seed;

pub use details::SqlxParkingDetailsReader;
pub use photos::SqlxParkingPhotoReader;
pub use seed::seed_mock;
pub use search::SqlxParkingSearchReader;
