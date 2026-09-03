//! Privacy & account-lifecycle infrastructure (plans/m6-privacy.md §6).

pub mod anonymize;
pub mod export;
pub mod policy;
pub mod policy_seed;
pub mod request;
pub mod retention;

pub use anonymize::SqlxAnonymizationRepository;
pub use export::SqlxExportRepository;
pub use policy::SqlxPolicyReader;
pub use policy_seed::{POLICY_LOCALES, POLICY_PLACEHOLDERS, fill_policy_placeholders, seed_policy};
pub use request::SqlxPrivacyRequestRepository;
pub use retention::SqlxRetentionRepository;
