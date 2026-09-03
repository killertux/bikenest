//! BikeNest infrastructure crate: persistence, config, ports impls.

pub mod auth;
pub mod community;
pub mod config;
pub mod db;
pub mod devdata;
pub mod email;
pub mod geocoding;
pub mod moderation;
pub mod parking;
pub mod photo;
pub mod privacy;
pub mod probe;
pub mod storage;
pub mod timezone;

pub use auth::{
    Argon2PasswordHasher, FakeOAuthProvider, InMemoryRateLimiter, RealTokenGenerator, SeedOutcome,
    SharedRateLimiter, SqlxAccountRepository, SqlxAuditLog, SqlxSessionStore, SqlxTokenStore,
    SystemClock, ValKeyRateLimiter, rate_limiter_from_env, seed_admin,
};
pub use community::{
    SqlxContributionHistoryReader, SqlxFavoriteRepository, SqlxParkingContributionRepository,
    SqlxReviewRepository, SqlxVerificationRepository,
};
pub use email::{
    CapturedEmail, FakeEmailProvider, ResendEmailProvider, SmtpEmailProvider, from_env as email_from_env,
};
pub use config::{Config, MapConfig, map_config_from_env};
pub use db::Db;
pub use geocoding::{FakeGeocoder, MapboxGeocoder, geocoder_from, geocoder_from_env};
pub use moderation::{SqlxAuditLogReader, SqlxModerationRepository, SqlxReportRepository};
pub use parking::{SqlxParkingDetailsReader, SqlxParkingPhotoReader, SqlxParkingSearchReader};
pub use privacy::{
    SqlxAnonymizationRepository, SqlxExportRepository, SqlxPolicyReader,
    SqlxPrivacyRequestRepository, SqlxRetentionRepository, seed_policy,
};
pub use photo::{LocalImageProcessor, SqlxPhotoRepository, SqlxReviewPhotosReader};
pub use storage::{SharedObjectStorage, S3ObjectStorage, storage_from_env};
pub use timezone::OfflineTimezoneResolver;
