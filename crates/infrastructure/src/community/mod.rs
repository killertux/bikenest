//! Community infrastructure (plans/m3-community.md §6).

pub mod contribution;
pub mod favorite;
pub mod history;
pub mod review;
pub mod verification;

pub use contribution::SqlxParkingContributionRepository;
pub use favorite::SqlxFavoriteRepository;
pub use history::SqlxContributionHistoryReader;
pub use review::SqlxReviewRepository;
pub use verification::SqlxVerificationRepository;
