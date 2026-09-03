//! M5 moderation service tests (pure, with fakes): report submission + rate
//! limit, the self-resolve guard, content hide/restore, parking invalidation and
//! proposal apply/reject. Persistence is faked; the orchestrating service rules
//! are what's under test.

use async_trait::async_trait;
use bikenest_application::{
    AuditFilter, AuditLog, AuditLogReader, AuditPage, AuthenticatedUser,
    ContributionHistoryReader, ContributionItem, ModerationDeps, ModerationError,
    ModerationRepository, ModerationService, PhotoKind, Proposal, ProposalApplication,
    Report, ReportRepository, RateLimitError, RateLimiter,
};
use bikenest_domain::{
    AccountState, ModerationState, ProposalKind, ProposalStatus, ReportOutcome, ReportState,
    ReportTargetType, Role, UserEmail, UserId,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct FakeReports {
    next_id: Arc<std::sync::atomic::AtomicI64>,
    rows: Arc<Mutex<HashMap<i64, Report>>>,
    claim_fails: Arc<Mutex<bool>>,
    resolve_fails: Arc<Mutex<bool>>,
}

impl Default for FakeReports {
    fn default() -> Self {
        Self {
            next_id: Arc::new(std::sync::atomic::AtomicI64::new(100)),
            rows: Arc::new(Mutex::new(HashMap::new())),
            claim_fails: Arc::new(Mutex::new(false)),
            resolve_fails: Arc::new(Mutex::new(false)),
        }
    }
}

impl FakeReports {
    fn new() -> Self {
        Self::default()
    }
    fn fail_claim(self) -> Self {
        *self.claim_fails.lock().unwrap() = true;
        self
    }
}

#[async_trait]
impl ReportRepository for FakeReports {
    async fn create(&self, r: &bikenest_application::NewReport) -> Result<i64, ModerationError> {
        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.rows.lock().unwrap().insert(
            id,
            Report {
                id,
                reporter_id: Some(r.reporter_id),
                target_type: r.target_type,
                target_id: r.target_id,
                reason: r.reason.clone(),
                description: Some(r.description.as_str().to_string()),
                state: ReportState::Open,
                claimed_by: None,
                resolved_by: None,
                resolution_note: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
        );
        Ok(id)
    }
    async fn list(&self, state: Option<ReportState>) -> Result<Vec<Report>, ModerationError> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .values()
            .filter(|r| state.map_or(true, |s| r.state == s))
            .cloned()
            .collect())
    }
    async fn get(&self, id: i64) -> Result<Option<Report>, ModerationError> {
        Ok(self.rows.lock().unwrap().get(&id).cloned())
    }
    async fn claim(&self, id: i64, moderator: UserId) -> Result<(), ModerationError> {
        if *self.claim_fails.lock().unwrap() {
            return Err(ModerationError::InvalidState);
        }
        let mut rows = self.rows.lock().unwrap();
        let r = rows.get_mut(&id).ok_or(ModerationError::NotFound)?;
        r.state = ReportState::UnderReview;
        r.claimed_by = Some(moderator);
        Ok(())
    }
    async fn resolve(
        &self,
        id: i64,
        moderator: UserId,
        note: &str,
        outcome: ReportOutcome,
    ) -> Result<(), ModerationError> {
        if *self.resolve_fails.lock().unwrap() {
            return Err(ModerationError::InvalidState);
        }
        let mut rows = self.rows.lock().unwrap();
        let r = rows.get_mut(&id).ok_or(ModerationError::NotFound)?;
        r.state = match outcome {
            ReportOutcome::Resolved => ReportState::Resolved,
            ReportOutcome::Dismissed => ReportState::Dismissed,
        };
        r.resolved_by = Some(moderator);
        r.resolution_note = Some(note.to_string());
        Ok(())
    }
}

#[derive(Clone)]
struct FakeModeration {
    target_exists: Arc<Mutex<bool>>,
    hidden_reviews: Arc<Mutex<Vec<i64>>>,
    restored_reviews: Arc<Mutex<Vec<i64>>>,
    hidden_photos: Arc<Mutex<Vec<(PhotoKind, i64)>>>,
    restored_photos: Arc<Mutex<Vec<(PhotoKind, i64)>>>,
    parking_states: Arc<Mutex<HashMap<i64, ModerationState>>>,
    pending_proposals: Arc<Mutex<Vec<Proposal>>>,
}

impl Default for FakeModeration {
    fn default() -> Self {
        Self {
            // Default to targets existing so happy-path submit tests pass.
            target_exists: Arc::new(Mutex::new(true)),
            hidden_reviews: Arc::new(Mutex::new(Vec::new())),
            restored_reviews: Arc::new(Mutex::new(Vec::new())),
            hidden_photos: Arc::new(Mutex::new(Vec::new())),
            restored_photos: Arc::new(Mutex::new(Vec::new())),
            parking_states: Arc::new(Mutex::new(HashMap::new())),
            pending_proposals: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl ModerationRepository for FakeModeration {
    async fn target_exists(&self, _t: ReportTargetType, _id: i64) -> Result<bool, ModerationError> {
        Ok(*self.target_exists.lock().unwrap())
    }
    async fn hide_review(&self, id: i64, _m: UserId) -> Result<(), ModerationError> {
        self.hidden_reviews.lock().unwrap().push(id);
        Ok(())
    }
    async fn restore_review(&self, id: i64, _m: UserId) -> Result<(), ModerationError> {
        self.restored_reviews.lock().unwrap().push(id);
        Ok(())
    }
    async fn hide_photo(&self, kind: PhotoKind, id: i64, _m: UserId) -> Result<(), ModerationError> {
        self.hidden_photos.lock().unwrap().push((kind, id));
        Ok(())
    }
    async fn restore_photo(&self, kind: PhotoKind, id: i64, _m: UserId) -> Result<(), ModerationError> {
        self.restored_photos.lock().unwrap().push((kind, id));
        Ok(())
    }
    async fn set_parking_state(
        &self,
        id: i64,
        _from: &[ModerationState],
        to: ModerationState,
        _m: UserId,
    ) -> Result<(), ModerationError> {
        self.parking_states.lock().unwrap().insert(id, to);
        Ok(())
    }
    async fn list_pending_proposals(&self) -> Result<Vec<Proposal>, ModerationError> {
        Ok(self.pending_proposals.lock().unwrap().clone())
    }
    async fn get_proposal(&self, id: i64) -> Result<Option<Proposal>, ModerationError> {
        Ok(self
            .pending_proposals
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.id == id)
            .cloned())
    }
    async fn approve_proposal(
        &self,
        id: i64,
        _m: UserId,
        applied: ProposalApplication,
    ) -> Result<(), ModerationError> {
        let mut props = self.pending_proposals.lock().unwrap();
        let p = props.iter_mut().find(|p| p.id == id).ok_or(ModerationError::NotFound)?;
        if p.status != ProposalStatus::Pending {
            return Err(ModerationError::InvalidState);
        }
        p.status = ProposalStatus::Approved;
        let _ = applied.kind();
        Ok(())
    }
    async fn reject_proposal(&self, id: i64, _m: UserId, _r: &str) -> Result<(), ModerationError> {
        let mut props = self.pending_proposals.lock().unwrap();
        let p = props.iter_mut().find(|p| p.id == id).ok_or(ModerationError::NotFound)?;
        if p.status != ProposalStatus::Pending {
            return Err(ModerationError::InvalidState);
        }
        p.status = ProposalStatus::Rejected;
        Ok(())
    }
}

#[derive(Clone, Default)]
struct FakeAuditReader;
#[async_trait]
impl AuditLogReader for FakeAuditReader {
    async fn list(&self, _f: AuditFilter) -> Result<AuditPage, bikenest_application::AuditError> {
        Ok(AuditPage { items: Vec::new(), next_cursor: None })
    }
}

#[derive(Clone, Default)]
struct FakeHistory;
#[async_trait]
impl ContributionHistoryReader for FakeHistory {
    async fn history(&self, _u: UserId) -> Result<Vec<ContributionItem>, bikenest_application::ContributionError> {
        Ok(Vec::new())
    }
}

#[derive(Default)]
struct FakeRate {
    allow: bool,
}
#[async_trait]
impl RateLimiter for FakeRate {
    async fn check(&self, _k: &str, _l: u32, _w: Duration) -> Result<bool, RateLimitError> {
        Ok(self.allow)
    }
}

#[derive(Clone, Default)]
struct FakeAudit(Arc<Mutex<Vec<String>>>);
#[async_trait]
impl AuditLog for FakeAudit {
    async fn record(&self, e: bikenest_application::AuditEvent) -> Result<(), bikenest_application::AuditError> {
        self.0.lock().unwrap().push(e.action);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn user(id: i64, roles: Vec<Role>) -> AuthenticatedUser {
    AuthenticatedUser {
        id: UserId(id),
        email: UserEmail::parse("u@example.com").unwrap(),
        display_name: None,
        account_state: AccountState::Active,
        is_verified: true,
        roles,
    }
}
fn moderator() -> AuthenticatedUser {
    user(2, vec![Role::Moderator])
}
fn reporter() -> AuthenticatedUser {
    user(1, vec![])
}

fn proposal(id: i64, kind: ProposalKind, proposed: serde_json::Value) -> Proposal {
    Proposal {
        id,
        location_id: 10,
        location_name: "Test".to_string(),
        proposer_id: Some(UserId(3)),
        base_version: 1,
        kind,
        proposed,
        status: ProposalStatus::Pending,
        created_at: chrono::Utc::now(),
    }
}

struct Harness {
    service: ModerationService,
    moderation: Arc<FakeModeration>,
    audit: Arc<FakeAudit>,
}

fn harness(allow_rate: bool) -> Harness {
    let reports = Arc::new(FakeReports::new());
    let moderation = Arc::new(FakeModeration::default());
    let audit = Arc::new(FakeAudit::default());
    let service = ModerationService::new(ModerationDeps {
        reports: Box::new(reports.as_ref().clone()),
        moderation: Box::new(moderation.as_ref().clone()),
        audit: Box::new(audit.as_ref().clone()),
        audit_reader: Box::new(FakeAuditReader),
        history: Box::new(FakeHistory),
        rate_limiter: Box::new(FakeRate { allow: allow_rate }),
    });
    Harness { service, moderation, audit }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn submit_report_happy_path_persists_open_and_audits() {
    let h = harness(true);
    h.moderation.target_exists.lock().unwrap().clone_from(&true);
    let id = h
        .service
        .submit_report(&reporter(), "1.2.3.4", ReportTargetType::Review, 5, "spam", Some("bad".to_string()))
        .await
        .unwrap();
    assert!(id > 0);
    let stored = h.service.get_report(&moderator(), id).await.unwrap().unwrap();
    assert_eq!(stored.state, ReportState::Open);
    assert_eq!(stored.reason, "spam");
    assert!(h.audit.0.lock().unwrap().contains(&"report.created".to_string()));
}

#[tokio::test]
async fn submit_report_rejects_unknown_reason() {
    let h = harness(true);
    h.moderation.target_exists.lock().unwrap().clone_from(&true);
    assert!(matches!(
        h.service.submit_report(&reporter(), "1.2.3.4", ReportTargetType::Parking, 1, "bogus", None).await,
        Err(ModerationError::InvalidReason)
    ));
}

#[tokio::test]
async fn submit_report_rejects_reason_not_allowed_for_target() {
    let h = harness(true);
    h.moderation.target_exists.lock().unwrap().clone_from(&true);
    // inappropriate_photo is not allowed on a parking location.
    assert!(matches!(
        h.service.submit_report(&reporter(), "1.2.3.4", ReportTargetType::Parking, 1, "inappropriate_photo", None).await,
        Err(ModerationError::InvalidReason)
    ));
}

#[tokio::test]
async fn submit_report_returns_target_not_found() {
    let h = harness(true);
    h.moderation.target_exists.lock().unwrap().clone_from(&false);
    assert!(matches!(
        h.service.submit_report(&reporter(), "1.2.3.4", ReportTargetType::Review, 999, "spam", None).await,
        Err(ModerationError::TargetNotFound)
    ));
}

#[tokio::test]
async fn submit_report_is_rate_limited() {
    let h = harness(false);
    h.moderation.target_exists.lock().unwrap().clone_from(&true);
    assert!(matches!(
        h.service.submit_report(&reporter(), "1.2.3.4", ReportTargetType::Parking, 1, "spam", None).await,
        Err(ModerationError::RateLimited)
    ));
}

#[tokio::test]
async fn resolve_own_report_is_self_resolve() {
    let h = harness(true);
    // A moderator is still the reporter → blocks at the self-resolve guard.
    let mod_reporter = user(1, vec![Role::Moderator]);
    let id = h
        .service
        .submit_report(&mod_reporter, "1.2.3.4", ReportTargetType::Parking, 1, "spam", None)
        .await
        .unwrap();
    let err = h
        .service
        .resolve_report(&mod_reporter, id, ReportOutcome::Resolved, "nope")
        .await
        .unwrap_err();
    assert!(matches!(err, ModerationError::SelfResolve));
}

#[tokio::test]
async fn claim_then_resolve_flow_audits_both() {
    let h = harness(true);
    let id = h
        .service
        .submit_report(&reporter(), "1.2.3.4", ReportTargetType::Parking, 1, "spam", None)
        .await
        .unwrap();
    h.service.claim_report(&moderator(), id).await.unwrap();
    h.service
        .resolve_report(&moderator(), id, ReportOutcome::Resolved, "removed listing")
        .await
        .unwrap();
    let stored = h.service.get_report(&moderator(), id).await.unwrap().unwrap();
    assert_eq!(stored.state, ReportState::Resolved);
    assert_eq!(stored.resolved_by, Some(UserId(2)));
    assert!(h.audit.0.lock().unwrap().contains(&"report.claimed".to_string()));
    assert!(h.audit.0.lock().unwrap().contains(&"report.resolved".to_string()));
}

#[tokio::test]
async fn claim_wrong_state_is_invalid_state() {
    let service = ModerationService::new(ModerationDeps {
        reports: Box::new(FakeReports::new().fail_claim()),
        moderation: Box::new(FakeModeration::default()),
        audit: Box::new(FakeAudit::default()),
        audit_reader: Box::new(FakeAuditReader),
        history: Box::new(FakeHistory),
        rate_limiter: Box::new(FakeRate { allow: true }),
    });
    assert!(matches!(
        service.claim_report(&moderator(), 1).await,
        Err(ModerationError::InvalidState)
    ));
}

#[tokio::test]
async fn hide_and_restore_review_audits() {
    let h = harness(true);
    h.service.hide_review(&moderator(), 7).await.unwrap();
    h.service.restore_review(&moderator(), 7).await.unwrap();
    assert_eq!(h.moderation.hidden_reviews.lock().unwrap().as_slice(), &[7]);
    assert_eq!(h.moderation.restored_reviews.lock().unwrap().as_slice(), &[7]);
    assert!(h.audit.0.lock().unwrap().contains(&"review.hidden".to_string()));
    assert!(h.audit.0.lock().unwrap().contains(&"review.restored".to_string()));
}

#[tokio::test]
async fn invalidate_and_restore_parking_audits() {
    let h = harness(true);
    h.service.invalidate_parking(&moderator(), 10).await.unwrap();
    // The fake only keeps the latest state (restore overwrites invalid), so we
    // assert on the audit trail + final state rather than both intermediate ones.
    h.service.restore_parking(&moderator(), 10).await.unwrap();
    let states = h.moderation.parking_states.lock().unwrap();
    assert_eq!(states.get(&10), Some(&ModerationState::Active));
    drop(states);
    assert!(h.audit.0.lock().unwrap().contains(&"parking.invalidated".to_string()));
    assert!(h.audit.0.lock().unwrap().contains(&"parking.restored".to_string()));
}

#[tokio::test]
async fn approve_and_reject_proposal() {
    let h = harness(true);
    h.moderation.pending_proposals.lock().unwrap().push(proposal(
        1,
        ProposalKind::MoveLocation,
        serde_json::json!({ "lat": -25.0, "lon": -49.0, "timezone": "America/Sao_Paulo" }),
    ));
    h.moderation.pending_proposals.lock().unwrap().push(proposal(
        2,
        ProposalKind::ChangeExistence,
        serde_json::json!({ "existence": "removed" }),
    ));

    let applied = ProposalApplication::from_proposed(
        ProposalKind::MoveLocation,
        &serde_json::json!({ "lat": -25.0, "lon": -49.0, "timezone": "America/Sao_Paulo" }),
    )
    .unwrap();
    h.service.approve_proposal(&moderator(), 1, applied).await.unwrap();
    h.service.reject_proposal(&moderator(), 2, "not right").await.unwrap();

    assert!(h.audit.0.lock().unwrap().contains(&"proposal.approved".to_string()));
    assert!(h.audit.0.lock().unwrap().contains(&"proposal.rejected".to_string()));
}

#[tokio::test]
async fn without_moderator_role_all_actions_are_unauthorized() {
    let h = harness(true);
    let plain = user(9, vec![]);
    assert!(matches!(h.service.claim_report(&plain, 1).await, Err(ModerationError::NotAuthorized)));
    assert!(matches!(h.service.hide_review(&plain, 1).await, Err(ModerationError::NotAuthorized)));
    assert!(matches!(h.service.invalidate_parking(&plain, 1).await, Err(ModerationError::NotAuthorized)));
    assert!(matches!(h.service.list_pending_proposals(&plain).await, Err(ModerationError::NotAuthorized)));
    assert!(matches!(h.service.list_reports(&plain, None).await, Err(ModerationError::NotAuthorized)));
}

#[tokio::test]
async fn submission_has_no_role_gate() {
    // Reporting is open to any authenticated user (even unverified brand-new).
    let h = harness(true);
    h.moderation.target_exists.lock().unwrap().clone_from(&true);
    let fresh = user(77, vec![]);
    assert!(h
        .service
        .submit_report(&fresh, "9.9.9.9", ReportTargetType::Parking, 1, "spam", None)
        .await
        .is_ok());
}


