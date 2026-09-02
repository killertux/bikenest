//! Moderation & reporting use cases (REQUIREMENTS §43–§45, §47).
//!
//! Ports + read models + [`ModerationService`]. Infrastructure implements the
//! ports; the web layer calls the service for every report / moderation action.
//! The report state machine, the self-resolve guard, the parking invalidation
//! invariant and the proposal-apply correctness all live here.

use crate::audit::{AuditEvent, AuditLog, AuditFilter, AuditLogReader, AuditPage};
use crate::photo::PhotoKind;
use crate::rate_limit::{RateLimitError, RateLimiter};
use async_trait::async_trait;
use bikenest_domain::{
    ModerationState, ProposalKind, ProposalStatus, ReportDescription, ReportOutcome, ReportState,
    ReportTargetType, Role, UserId, is_known_report_reason, reason_allowed_for,
};
use chrono::{DateTime, Utc};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ModerationError {
    #[error("you are not permitted to perform this action")]
    NotAuthorized,
    #[error("you cannot resolve your own report")]
    SelfResolve,
    #[error("report not found")]
    NotFound,
    #[error("target could not be found")]
    TargetNotFound,
    #[error("that report is not in the required state")]
    InvalidState,
    #[error("invalid report reason")]
    InvalidReason,
    #[error("invalid input: {0}")]
    InvalidField(String),
    #[error("too many reports, try again later")]
    RateLimited,
    #[error("internal error")]
    Internal,
}

impl From<RateLimitError> for ModerationError {
    fn from(_: RateLimitError) -> Self {
        ModerationError::RateLimited
    }
}

impl From<crate::audit::AuditError> for ModerationError {
    fn from(_: crate::audit::AuditError) -> Self {
        ModerationError::Internal
    }
}

impl From<bikenest_domain::DomainError> for ModerationError {
    fn from(e: bikenest_domain::DomainError) -> Self {
        ModerationError::InvalidField(e.to_string())
    }
}

impl From<crate::ports::ReaderError> for ModerationError {
    fn from(_: crate::ports::ReaderError) -> Self {
        ModerationError::Internal
    }
}

// ---------------------------------------------------------------------------
// Read models
// ---------------------------------------------------------------------------

/// A validated new report, ready to persist.
#[derive(Debug, Clone)]
pub struct NewReport {
    pub reporter_id: UserId,
    pub target_type: ReportTargetType,
    pub target_id: i64,
    pub reason: String,
    pub description: ReportDescription,
}

/// A report as read from the store (for the queue + detail + resolution).
#[derive(Debug, Clone)]
pub struct Report {
    pub id: i64,
    pub reporter_id: UserId,
    pub target_type: ReportTargetType,
    pub target_id: i64,
    pub reason: String,
    pub description: Option<String>,
    pub state: ReportState,
    pub claimed_by: Option<UserId>,
    pub resolved_by: Option<UserId>,
    pub resolution_note: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A pending (or resolved) proposal in the moderator queue, with its location
/// name for context.
#[derive(Debug, Clone)]
pub struct Proposal {
    pub id: i64,
    pub location_id: i64,
    pub location_name: String,
    pub proposer_id: UserId,
    pub base_version: i64,
    pub kind: ProposalKind,
    pub proposed: serde_json::Value,
    pub status: ProposalStatus,
    pub created_at: DateTime<Utc>,
}

/// Per-attribute change tally for a proposal's "current vs proposed" context.
/// The web layer derives a diff from `proposed`; this struct is the typed shape
/// the service validates before applying.
#[derive(Debug, Clone)]
pub enum ProposalApplication {
    MoveLocation {
        lat: f64,
        lon: f64,
        timezone: chrono_tz::Tz,
    },
    /// `true` = exists/restore to `ACTIVE`; `false` = removed → `REMOVED`.
    ChangeExistence { exists: bool },
}

impl ProposalApplication {
    /// Parse a proposal's stored `proposed` JSONB into the typed application.
    pub fn from_proposed(kind: ProposalKind, proposed: &serde_json::Value) -> Result<Self, ModerationError> {
        match kind {
            ProposalKind::MoveLocation => {
                let lat = proposed
                    .get("lat")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| ModerationError::InvalidField("lat is required".to_string()))?;
                let lon = proposed
                    .get("lon")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| ModerationError::InvalidField("lon is required".to_string()))?;
                let tz_raw = proposed
                    .get("timezone")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ModerationError::InvalidField("timezone is required".to_string()))?;
                let timezone = tz_raw
                    .parse()
                    .map_err(|_| ModerationError::InvalidField("invalid timezone".to_string()))?;
                Ok(ProposalApplication::MoveLocation {
                    lat,
                    lon,
                    timezone,
                })
            }
            ProposalKind::ChangeExistence => {
                let exists = match proposed.get("existence").and_then(|v| v.as_str()) {
                    Some("removed") => false,
                    Some("exists") => true,
                    Some(other) => {
                        return Err(ModerationError::InvalidField(format!(
                            "unknown existence: {other}"
                        )));
                    }
                    None => return Err(ModerationError::InvalidField("existence is required".to_string())),
                };
                Ok(ProposalApplication::ChangeExistence { exists })
            }
        }
    }

    pub fn kind(&self) -> ProposalKind {
        match self {
            ProposalApplication::MoveLocation { .. } => ProposalKind::MoveLocation,
            ProposalApplication::ChangeExistence { .. } => ProposalKind::ChangeExistence,
        }
    }
}

// ---------------------------------------------------------------------------
// Ports
// ---------------------------------------------------------------------------

/// The report CRUD + state transitions.
#[async_trait]
pub trait ReportRepository: Send + Sync {
    async fn create(&self, r: &NewReport) -> Result<i64, ModerationError>;
    async fn list(&self, state: Option<ReportState>) -> Result<Vec<Report>, ModerationError>;
    /// Returns the report incl. its `reporter_id` (needed by the self-resolve guard).
    async fn get(&self, id: i64) -> Result<Option<Report>, ModerationError>;
    /// `OPEN → UNDER_REVIEW`, setting `claimed_by`/`updated_at`. 0 rows → `InvalidState`.
    async fn claim(&self, id: i64, moderator: UserId) -> Result<(), ModerationError>;
    /// `UNDER_REVIEW → RESOLVED|DISMISSED`, setting `resolved_by`/note/`updated_at`.
    async fn resolve(
        &self,
        id: i64,
        moderator: UserId,
        note: &str,
        outcome: ReportOutcome,
    ) -> Result<(), ModerationError>;
}

/// The moderation repository: target existence checks and the content/flip
/// actions (hide/restore/invalidate/proposal-apply).
#[async_trait]
pub trait ModerationRepository: Send + Sync {
    /// One lookup across the four target tables (polymorphic `report.target_id`).
    async fn target_exists(&self, target_type: ReportTargetType, target_id: i64) -> Result<bool, ModerationError>;
    /// `ACTIVE → HIDDEN` an existing review. 0 rows → `InvalidState`.
    async fn hide_review(&self, id: i64, moderator: UserId) -> Result<(), ModerationError>;
    /// `HIDDEN → ACTIVE` a review. 0 rows → `InvalidState`.
    async fn restore_review(&self, id: i64, moderator: UserId) -> Result<(), ModerationError>;
    /// `APPROVED → HIDDEN` a photo (§44). 0 rows → `InvalidState`.
    async fn hide_photo(&self, kind: PhotoKind, id: i64, moderator: UserId) -> Result<(), ModerationError>;
    /// `HIDDEN → APPROVED` a photo. 0 rows → `InvalidState`.
    async fn restore_photo(&self, kind: PhotoKind, id: i64, moderator: UserId) -> Result<(), ModerationError>;
    /// Set a parking location's moderation state (only from the allowed `from`
    /// states), bump `version`, append a ``moderation`` revision — one tx.
    async fn set_parking_state(
        &self,
        id: i64,
        from: &[ModerationState],
        to: ModerationState,
        moderator: UserId,
    ) -> Result<(), ModerationError>;
    async fn list_pending_proposals(&self) -> Result<Vec<Proposal>, ModerationError>;
    async fn get_proposal(&self, id: i64) -> Result<Option<Proposal>, ModerationError>;
    /// Apply the proposal's change (or the moderator's adjusted values), bump
    /// version + append a `moderation` revision, set status `APPROVED`, and
    /// supersede older PENDING proposals on the same location — one tx.
    async fn approve_proposal(
        &self,
        id: i64,
        moderator: UserId,
        applied: ProposalApplication,
    ) -> Result<(), ModerationError>;
    /// Set a proposal to `REJECTED` with a reason; no live change.
    async fn reject_proposal(&self, id: i64, moderator: UserId, reason: &str) -> Result<(), ModerationError>;
}

// ---------------------------------------------------------------------------
// Rate-limit defaults (§45). Keys `report:create:user:{id}` and
// `report:create:ip:{ip}`. Moderator actions are audited, not rate-limited.
// ---------------------------------------------------------------------------

const REPORT_CREATE_USER_LIMIT: u32 = 10;
const REPORT_CREATE_IP_LIMIT: u32 = 20;

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

// ---------------------------------------------------------------------------
// ModerationService
// ---------------------------------------------------------------------------

/// Everything the moderation use cases depend on, bundled for construction.
pub struct ModerationDeps {
    pub reports: Box<dyn ReportRepository>,
    pub moderation: Box<dyn ModerationRepository>,
    pub audit: Box<dyn AuditLog>,
    pub audit_reader: Box<dyn AuditLogReader>,
    pub history: Box<dyn crate::community::ContributionHistoryReader>,
    pub rate_limiter: Box<dyn RateLimiter>,
}

pub struct ModerationService {
    deps: ModerationDeps,
}

impl ModerationService {
    pub fn new(deps: ModerationDeps) -> Self {
        Self { deps }
    }

    fn require_moderator(&self, user: &crate::auth::AuthenticatedUser) -> Result<(), ModerationError> {
        if user.has_role(Role::Moderator) || user.has_role(Role::Admin) {
            Ok(())
        } else {
            Err(ModerationError::NotAuthorized)
        }
    }

    fn require_admin(&self, user: &crate::auth::AuthenticatedUser) -> Result<(), ModerationError> {
        if user.has_role(Role::Admin) {
            Ok(())
        } else {
            Err(ModerationError::NotAuthorized)
        }
    }

    async fn allowed(
        &self,
        key: &str,
        limit: u32,
        window: Duration,
    ) -> Result<(), ModerationError> {
        if self.deps.rate_limiter.check(key, limit, window).await? {
            Ok(())
        } else {
            Err(ModerationError::RateLimited)
        }
    }

    // -----------------------------------------------------------------------
    // Reports (§43/§45/§103)
    // -----------------------------------------------------------------------

    /// Submit a report. Gated to *authenticated* users (not verified — reporting
    /// abuse must work for a brand-new account); the target must exist; the reason
    /// must be known and allowed for the target; the description is optional and
    /// capped; the action is rate-limited.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_report(
        &self,
        user: &crate::auth::AuthenticatedUser,
        ip: &str,
        target_type: ReportTargetType,
        target_id: i64,
        reason: &str,
        description: Option<String>,
    ) -> Result<i64, ModerationError> {
        self.allowed(
            &format!("report:create:user:{}", user.id.0),
            REPORT_CREATE_USER_LIMIT,
            DAY,
        )
        .await?;
        self.allowed(&format!("report:create:ip:{ip}"), REPORT_CREATE_IP_LIMIT, DAY).await?;

        if !is_known_report_reason(reason) {
            return Err(ModerationError::InvalidReason);
        }
        if !reason_allowed_for(target_type, reason) {
            return Err(ModerationError::InvalidReason);
        }
        let description = match description {
            Some(raw) if !raw.trim().is_empty() => ReportDescription::new(&raw)?,
            _ => ReportDescription::new("")?,
        };

        if !self.deps.moderation.target_exists(target_type, target_id).await? {
            return Err(ModerationError::TargetNotFound);
        }

        let new = NewReport {
            reporter_id: user.id,
            target_type,
            target_id,
            reason: reason.to_string(),
            description,
        };
        let id = self.deps.reports.create(&new).await?;
        self.audit(
            Some(user.id),
            "report.created",
            "report",
            id.to_string(),
            serde_json::json!({ "target_type": target_type.as_code(), "target_id": target_id, "reason": reason }),
        )
        .await?;
        Ok(id)
    }

    /// The report queue. `require_moderator`. Returns `None` to list all states.
    pub async fn list_reports(
        &self,
        moderator: &crate::auth::AuthenticatedUser,
        state: Option<ReportState>,
    ) -> Result<Vec<Report>, ModerationError> {
        self.require_moderator(moderator)?;
        self.deps.reports.list(state).await
    }

    pub async fn get_report(
        &self,
        moderator: &crate::auth::AuthenticatedUser,
        id: i64,
    ) -> Result<Option<Report>, ModerationError> {
        self.require_moderator(moderator)?;
        self.deps.reports.get(id).await
    }

    /// Claim an open report (`OPEN → UNDER_REVIEW`) and record who claimed it.
    pub async fn claim_report(
        &self,
        moderator: &crate::auth::AuthenticatedUser,
        id: i64,
    ) -> Result<(), ModerationError> {
        self.require_moderator(moderator)?;
        self.deps.reports.claim(id, moderator.id).await?;
        self.audit(
            Some(moderator.id),
            "report.claimed",
            "report",
            id.to_string(),
            serde_json::json!({}),
        )
        .await?;
        Ok(())
    }

    /// Resolve or dismiss a claimed report. **Server-side self-resolve guard**:
    /// a user cannot resolve a report they submitted (§43).
    pub async fn resolve_report(
        &self,
        moderator: &crate::auth::AuthenticatedUser,
        id: i64,
        outcome: ReportOutcome,
        note: &str,
    ) -> Result<(), ModerationError> {
        self.require_moderator(moderator)?;
        let report = self
            .deps
            .reports
            .get(id)
            .await?
            .ok_or(ModerationError::NotFound)?;
        if report.reporter_id == moderator.id {
            return Err(ModerationError::SelfResolve);
        }
        self.deps
            .reports
            .resolve(id, moderator.id, note, outcome)
            .await?;
        // A "Resolved" report means the reported content was inappropriate, so
        // the target is hidden/invalidated too (idempotent — already-hidden is fine).
        if outcome == ReportOutcome::Resolved {
            self.hide_resolved_target(&report, moderator.id).await?;
        }
        let action = match outcome {
            ReportOutcome::Resolved => "report.resolved",
            ReportOutcome::Dismissed => "report.dismissed",
        };
        self.audit(
            Some(moderator.id),
            action,
            "report",
            id.to_string(),
            serde_json::json!({ "target_type": report.target_type.as_code(), "target_id": report.target_id }),
        )
        .await?;
        Ok(())
    }

    /// Hide/invalidate the target of a resolved report. Tolerant of a target
    /// that is already hidden (the moderator may resolve after hiding separately).
    async fn hide_resolved_target(
        &self,
        report: &Report,
        moderator: UserId,
    ) -> Result<(), ModerationError> {
        let res = match report.target_type {
            ReportTargetType::Review => {
                self.deps.moderation.hide_review(report.target_id, moderator).await
            }
            ReportTargetType::ParkingPhoto => {
                self.deps.moderation.hide_photo(PhotoKind::Parking, report.target_id, moderator).await
            }
            ReportTargetType::ReviewPhoto => {
                self.deps.moderation.hide_photo(PhotoKind::Review, report.target_id, moderator).await
            }
            ReportTargetType::Parking => {
                self.deps.moderation.set_parking_state(
                    report.target_id,
                    &[ModerationState::Active],
                    ModerationState::Invalid,
                    moderator,
                )
                .await
            }
        };
        match res {
            Ok(()) => Ok(()),
            // Already in the target state (e.g. already hidden) → not an error.
            Err(ModerationError::InvalidState) => Ok(()),
            // Target deleted after the report was filed → the resolve still stands.
            Err(ModerationError::NotFound) => Ok(()),
            Err(e) => Err(e),
        }
    }

    // -----------------------------------------------------------------------
    // Content moderation actions (§44)
    // -----------------------------------------------------------------------

    pub async fn hide_review(
        &self,
        moderator: &crate::auth::AuthenticatedUser,
        id: i64,
    ) -> Result<(), ModerationError> {
        self.require_moderator(moderator)?;
        self.deps.moderation.hide_review(id, moderator.id).await?;
        self.audit(
            Some(moderator.id),
            "review.hidden",
            "review",
            id.to_string(),
            serde_json::json!({}),
        )
        .await?;
        Ok(())
    }

    pub async fn restore_review(
        &self,
        moderator: &crate::auth::AuthenticatedUser,
        id: i64,
    ) -> Result<(), ModerationError> {
        self.require_moderator(moderator)?;
        self.deps.moderation.restore_review(id, moderator.id).await?;
        self.audit(
            Some(moderator.id),
            "review.restored",
            "review",
            id.to_string(),
            serde_json::json!({}),
        )
        .await?;
        Ok(())
    }

    pub async fn hide_photo(
        &self,
        moderator: &crate::auth::AuthenticatedUser,
        kind: PhotoKind,
        id: i64,
    ) -> Result<(), ModerationError> {
        self.require_moderator(moderator)?;
        self.deps.moderation.hide_photo(kind, id, moderator.id).await?;
        self.audit(
            Some(moderator.id),
            "photo.hidden",
            kind.as_code(),
            id.to_string(),
            serde_json::json!({}),
        )
        .await?;
        Ok(())
    }

    pub async fn restore_photo(
        &self,
        moderator: &crate::auth::AuthenticatedUser,
        kind: PhotoKind,
        id: i64,
    ) -> Result<(), ModerationError> {
        self.require_moderator(moderator)?;
        self.deps.moderation.restore_photo(kind, id, moderator.id).await?;
        self.audit(
            Some(moderator.id),
            "photo.restored",
            kind.as_code(),
            id.to_string(),
            serde_json::json!({}),
        )
        .await?;
        Ok(())
    }

    /// Invalidate a location: `ACTIVE → INVALID` (takedown, restorable).
    pub async fn invalidate_parking(
        &self,
        moderator: &crate::auth::AuthenticatedUser,
        id: i64,
    ) -> Result<(), ModerationError> {
        self.require_moderator(moderator)?;
        self.deps
            .moderation
            .set_parking_state(id, &[ModerationState::Active], ModerationState::Invalid, moderator.id)
            .await?;
        self.audit(
            Some(moderator.id),
            "parking.invalidated",
            "parking_location",
            id.to_string(),
            serde_json::json!({}),
        )
        .await?;
        Ok(())
    }

    /// Restore a location: `INVALID|REMOVED → ACTIVE`.
    pub async fn restore_parking(
        &self,
        moderator: &crate::auth::AuthenticatedUser,
        id: i64,
    ) -> Result<(), ModerationError> {
        self.require_moderator(moderator)?;
        self.deps
            .moderation
            .set_parking_state(
                id,
                &[ModerationState::Invalid, ModerationState::Removed],
                ModerationState::Active,
                moderator.id,
            )
            .await?;
        self.audit(
            Some(moderator.id),
            "parking.restored",
            "parking_location",
            id.to_string(),
            serde_json::json!({}),
        )
        .await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Proposals (§37/§44)
    // -----------------------------------------------------------------------

    pub async fn list_pending_proposals(
        &self,
        moderator: &crate::auth::AuthenticatedUser,
    ) -> Result<Vec<Proposal>, ModerationError> {
        self.require_moderator(moderator)?;
        self.deps.moderation.list_pending_proposals().await
    }

    pub async fn get_proposal(
        &self,
        moderator: &crate::auth::AuthenticatedUser,
        id: i64,
    ) -> Result<Option<Proposal>, ModerationError> {
        self.require_moderator(moderator)?;
        self.deps.moderation.get_proposal(id).await
    }

    /// Approve a proposal. `applied` carries the values to apply (normally the
    /// proposal's own `proposed`, overridable by the approve form for "modify").
    pub async fn approve_proposal(
        &self,
        moderator: &crate::auth::AuthenticatedUser,
        id: i64,
        applied: ProposalApplication,
    ) -> Result<(), ModerationError> {
        self.require_moderator(moderator)?;
        let kind = applied.kind();
        self.deps
            .moderation
            .approve_proposal(id, moderator.id, applied)
            .await?;
        self.audit(
            Some(moderator.id),
            "proposal.approved",
            "parking_proposal",
            id.to_string(),
            serde_json::json!({ "kind": kind.as_code() }),
        )
        .await?;
        Ok(())
    }

    /// Reject a proposal with a reason (no live change).
    pub async fn reject_proposal(
        &self,
        moderator: &crate::auth::AuthenticatedUser,
        id: i64,
        reason: &str,
    ) -> Result<(), ModerationError> {
        self.require_moderator(moderator)?;
        self.deps
            .moderation
            .reject_proposal(id, moderator.id, reason)
            .await?;
        self.audit(
            Some(moderator.id),
            "proposal.rejected",
            "parking_proposal",
            id.to_string(),
            serde_json::json!({ "reason": reason }),
        )
        .await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Audit viewer (§47) + contribution inspection (§44)
    // -----------------------------------------------------------------------

    /// The admin audit-log viewer. ADMIN-only.
    pub async fn list_audit_events(
        &self,
        admin: &crate::auth::AuthenticatedUser,
        filter: AuditFilter,
    ) -> Result<AuditPage, ModerationError> {
        self.require_admin(admin)?;
        self.deps.audit_reader.list(filter).await.map_err(ModerationError::from)
    }

    /// Inspect a target user's contribution history (MODERATOR/ADMIN) — the C5
    /// aggregation scoped to that user.
    pub async fn user_contribution_history(
        &self,
        moderator: &crate::auth::AuthenticatedUser,
        target: UserId,
    ) -> Result<Vec<crate::community::ContributionItem>, ModerationError> {
        self.require_moderator(moderator)?;
        self.deps
            .history
            .history(target)
            .await
            .map_err(crate::community::ContributionError::from)
            .map_err(|_| ModerationError::Internal)
    }

    async fn audit(
        &self,
        actor: Option<UserId>,
        action: &str,
        target_type: &str,
        target_id: impl Into<String>,
        metadata: serde_json::Value,
    ) -> Result<(), ModerationError> {
        self.deps
            .audit
            .record(AuditEvent::new(
                actor,
                action,
                target_type,
                target_id,
                "success",
                metadata,
            ))
            .await?;
        Ok(())
    }
}
