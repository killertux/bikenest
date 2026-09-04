//! Parking persistence (REQUIREMENTS §24–§32).

pub mod details;
pub mod photos;
pub mod search;
pub mod seed;
pub mod sitemap;

pub use details::SqlxParkingDetailsReader;
pub use photos::SqlxParkingPhotoReader;
pub use search::SqlxParkingSearchReader;
pub use seed::seed_mock;
pub use sitemap::SqlxSitemapReader;
