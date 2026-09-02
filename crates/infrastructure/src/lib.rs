//! BikeNest infrastructure crate: persistence, config, ports impls.

pub mod auth;
pub mod community;
pub mod config;
pub mod db;
pub mod devdata;
pub mod email;
pub mod geocoding;
pub mod parking;
pub mod photo;
pub mod probe;
pub mod storage;
pub mod timezone;

pub use auth::{
    Argon2PasswordHasher, FakeOAuthProvider, InMemoryRateLimiter, RealTokenGenerator, SeedOutcome,
    SqlxAccountRepository, SqlxAuditLog, SqlxSessionStore, SqlxTokenStore, SystemClock, seed_admin,
};
pub use community::{
    SqlxContributionHistoryReader, SqlxFavoriteRepository, SqlxParkingContributionRepository,
    SqlxReviewRepository, SqlxVerificationRepository,
};
pub use email::{
    CapturedEmail, FakeEmailProvider, ResendEmailProvider, SmtpEmailProvider, from_env as email_from_env,
};
pub use config::Config;
pub use db::Db;
pub use geocoding::FakeGeocoder;
pub use parking::{SqlxParkingDetailsReader, SqlxParkingPhotoReader, SqlxParkingSearchReader};
pub use photo::{LocalImageProcessor, SqlxPhotoRepository};
pub use storage::{LocalDiskStorage, MEDIA_BASE_PATH};
pub use timezone::OfflineTimezoneResolver;
