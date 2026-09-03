//! BikeNest application crate: use cases and ports.
//!
//! Depends only on the domain. Infrastructure implements the ports defined
//! here (dependency points inward, REQUIREMENTS §3/§5).

use async_trait::async_trait;
use bikenest_domain::DomainError;

pub mod audit;
pub mod auth;
pub mod community;
pub mod email;
pub mod moderation;
pub mod photo;
pub mod ports;
pub mod privacy;
pub mod rate_limit;
pub mod search;
pub mod storage;
pub mod timezone;

pub use audit::{AuditError, AuditEvent, AuditLog, AuditLogReader, AuditFilter, AuditPage, AuditStoredEvent};
pub use auth::{
    AccountRepository, AuthError, AuthenticatedUser, AuthService, Clock, IdentityRecord,
    LoginOutcome, NewAccount, OAuthProvider, PasswordHasher, ResolvedSession, Session, SessionStore,
    TokenGenerator, TokenStore,
};
pub use email::{EmailError, EmailProvider, OutboundEmail};
pub use moderation::{
    ModerationDeps, ModerationError, ModerationRepository, ModerationService, NewReport,
    Proposal, ProposalApplication, Report, ReportRepository,
};
pub use ports::{
    CostFilter, Cursor, Filters, FreshnessConfig, GeoHit, GeocodeError, Geocoder,
    ParkingDetailsReader, ParkingPhotoReader, ParkingSearchReader, ParkingSummary, ReaderError,
    ReviewPhotosReader, SearchInput, SearchPage, SearchRequest, Sort, StoredPhoto,
};
pub use community::{
    AddParkingLocationOutcome, AttributeSummary, CommunityParkingDetails, ContributionDeps,
    ContributionError, ContributionHistoryReader, ContributionItem, ContributionService,
    DuplicateCandidate, FavoriteRepository, NewParkingLocation, NewProposal, NewVerification,
    ParkingContributionRepository, ParkingEdit, Reason, Review, ReviewRepository,
    VerificationRepository, recommendation_reasons,
};
pub use photo::{
    ImageProcessor, NewPendingPhoto, PendingPhoto, PhotoDeps, PhotoError, PhotoForModeration,
    PhotoKind, PhotoRepository, PhotoService, PhotoTarget, ProcessedImage, RejectedPhoto,
    UploadedPhoto,
};
pub use privacy::{
    AnonymizationReport, AnonymizationRepository, Export, ExportAccount, ExportDownload,
    ExportFavorite, ExportPayload, ExportPhoto, ExportProvider, ExportReport, ExportRepository,
    ExportProposal, ExportRequested, ExportReview, ExportReviewRevision, ExportSession,
    ExportVerification, NewExport, NewPrivacyRequest, PolicyDocument, PolicyReader, PrivacyDeps,
    PrivacyError, PrivacyRequest, PrivacyRequestRepository, PrivacyService, RetentionConfig,
    RetentionJob, RetentionRepository, RetentionStep, RetentionSummary,
};
pub use rate_limit::{RateLimitError, RateLimiter};
pub use storage::{ObjectStorage, PutObject, StorageError};
pub use timezone::{TimezoneError, TimezoneResolver};
pub use search::{
    recommendation_score, DetailsError, GetParkingDetails, ParkingDetailsView, RecommendationConfig,
    SearchError, SearchParking, DEFAULT_RECOMMENDATION_CONFIG,
};

/// Port: probe a required dependency (initially, the database).
#[async_trait]
pub trait DatabaseProbe: Send + Sync {
    /// Returns `Ok(())` when the dependency is reachable and responsive.
    async fn ping(&self) -> Result<(), ProbeError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    /// The dependency is unreachable / timed out (readiness must report
    /// "dependency down", distinct from an application bug — §87).
    #[error("dependency unavailable")]
    Unavailable,
    /// An unexpected error on our side (maps to 5xx app error).
    #[error("probe failed unexpectedly")]
    Unexpected,
}

impl From<DomainError> for ProbeError {
    fn from(_: DomainError) -> Self {
        // Domain errors are not expected from probes; mapped for completeness.
        ProbeError::Unexpected
    }
}

/// Outcome of the readiness use case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    DependencyDown,
    AppError,
}

/// Use case: can the application serve requests and access its dependencies?
pub struct CheckReadiness<P> {
    probe: P,
}

impl<P: DatabaseProbe> CheckReadiness<P> {
    pub fn new(probe: P) -> Self {
        Self { probe }
    }

    pub async fn execute(&self) -> Readiness {
        match self.probe.ping().await {
            Ok(()) => Readiness::Ready,
            Err(ProbeError::Unavailable) => Readiness::DependencyDown,
            Err(ProbeError::Unexpected) => Readiness::AppError,
        }
    }
}
