//! Accounts & authentication infrastructure: the real SQLx persistent stores,
//! the argon2id password hasher, token/clock impls, and the dev fakes
//! (email, OAuth, in-memory rate limiter). See plans/m2-accounts-auth.md §6.

pub mod account_repo;
pub mod audit;
pub mod clock;
pub mod hash;
pub mod oauth;
pub mod password;
pub mod rate_limit;
pub mod seed;
pub mod session_store;
pub mod token;
pub mod token_store;

pub use account_repo::SqlxAccountRepository;
pub use audit::SqlxAuditLog;
pub use clock::SystemClock;
pub use oauth::FakeOAuthProvider;
pub use password::Argon2PasswordHasher;
pub use rate_limit::{InMemoryRateLimiter, SharedRateLimiter, ValKeyRateLimiter, rate_limiter_from_env};
pub use seed::{seed_admin, SeedOutcome};
pub use session_store::SqlxSessionStore;
pub use token::RealTokenGenerator;
pub use token_store::SqlxTokenStore;
