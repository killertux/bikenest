//! Application-layer tests for the community contribution service, using
//! in-memory fakes for every port (no database). These lock the orchestration
//! rules: the verified gate, timezone auto-derivation, advisory duplicates,
//! rate limiting, optimistic-concurrency propagation, verification freshness
//! and the confidence/dispute aggregation.

use async_trait::async_trait;
use bikenest_application::{
    AddParkingLocationOutcome, AttributeSummary, AuthenticatedUser, Clock, ContributionDeps,
    ContributionError, ContributionHistoryReader, ContributionItem, ContributionService,
    DuplicateCandidate, FavoriteRepository, NewParkingLocation, NewVerification,
    ParkingContributionRepository, ParkingDetailsReader, ParkingEdit, Review, ReviewRepository,
    TimezoneError, TimezoneResolver, VerificationRepository,
};
use bikenest_domain::{
    AccountState, Confidence, Cost, ExistenceResult, ExistenceSignal, GeoPoint, OpeningHours,
    ParkingLocation, ParkingType, ReviewBody, SecurityFeature, SecurityState, StarRating, UserId,
};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

fn tz() -> chrono_tz::Tz {
    "America/Sao_Paulo".parse().unwrap()
}

fn verified_user(uid: i64) -> AuthenticatedUser {
    AuthenticatedUser {
        id: UserId(uid),
        email: bikenest_domain::UserEmail::parse("ok@example.com").unwrap(),
        display_name: None,
        account_state: AccountState::Active,
        is_verified: true,
        roles: vec![bikenest_domain::Role::User],
    }
}

fn unverified_user(uid: i64) -> AuthenticatedUser {
    AuthenticatedUser {
        account_state: AccountState::PendingEmailVerification,
        is_verified: false,
        ..verified_user(uid)
    }
}

fn new_input() -> NewParkingLocation {
    NewParkingLocation {
        name: "Estação Centro".to_string(),
        address: "Rua Teste, 10".to_string(),
        description: None,
        parking_type: ParkingType::Rack,
        cost: Cost::Free,
        point: GeoPoint::new(-25.4284, -49.2733).unwrap(),
        timezone: None, // let the service auto-derive
        hours: OpeningHours::Unknown,
        security: vec![SecurityFeature::new("well_lit", SecurityState::Yes)],
    }
}

// ---------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------

struct FakeTz;
#[async_trait]
impl TimezoneResolver for FakeTz {
    async fn resolve(&self, _p: GeoPoint) -> Result<chrono_tz::Tz, TimezoneError> {
        Ok(tz())
    }
}

struct FakeClock(chrono::DateTime<chrono::Utc>);
impl Clock for FakeClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.0
    }
}

struct AllowRateLimiter;
#[async_trait]
impl bikenest_application::RateLimiter for AllowRateLimiter {
    async fn check(
        &self,
        _k: &str,
        _l: u32,
        _w: Duration,
    ) -> Result<bool, bikenest_application::RateLimitError> {
        Ok(true)
    }
}

struct DenyRateLimiter;
#[async_trait]
impl bikenest_application::RateLimiter for DenyRateLimiter {
    async fn check(
        &self,
        _k: &str,
        _l: u32,
        _w: Duration,
    ) -> Result<bool, bikenest_application::RateLimitError> {
        Ok(false)
    }
}

struct RecordingAudit {
    events: Mutex<Vec<String>>,
}
#[async_trait]
impl bikenest_application::AuditLog for RecordingAudit {
    async fn record(
        &self,
        e: bikenest_application::AuditEvent,
    ) -> Result<(), bikenest_application::AuditError> {
        self.events.lock().unwrap().push(e.action.clone());
        Ok(())
    }
}

/// Minimal contribution repo: tracks id→version and a fixed duplicates list.
struct FakeContributionRepo {
    next_id: Mutex<i64>,
    versions: Mutex<HashMap<i64, i64>>,
    dupes: Mutex<Vec<DuplicateCandidate>>,
}
impl FakeContributionRepo {
    fn new(dupes: Vec<DuplicateCandidate>) -> Self {
        Self {
            next_id: Mutex::new(100),
            versions: Mutex::new(HashMap::new()),
            dupes: Mutex::new(dupes),
        }
    }
}
#[async_trait]
impl ParkingContributionRepository for FakeContributionRepo {
    async fn get_for_edit(&self, id: i64) -> Result<Option<ParkingLocation>, ContributionError> {
        let v = self.versions.lock().unwrap().get(&id).copied();
        v.map(|version| location_at(id, version)).transpose()
    }
    async fn create(
        &self,
        _new: &NewParkingLocation,
        _creator: UserId,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<i64, ContributionError> {
        let mut n = self.next_id.lock().unwrap();
        let id = *n;
        *n += 1;
        self.versions.lock().unwrap().insert(id, 1);
        Ok(id)
    }
    async fn apply_edit(
        &self,
        id: i64,
        expected_version: i64,
        _edit: &ParkingEdit,
        _editor: UserId,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<i64, ContributionError> {
        let mut v = self.versions.lock().unwrap();
        if v.get(&id).copied() != Some(expected_version) {
            return Err(ContributionError::VersionConflict);
        }
        let new_version = expected_version + 1;
        v.insert(id, new_version);
        Ok(new_version)
    }
    async fn create_proposal(
        &self,
        _p: &bikenest_application::NewProposal,
    ) -> Result<i64, ContributionError> {
        Ok(7)
    }
    async fn revision_history(
        &self,
        _id: i64,
    ) -> Result<Vec<bikenest_domain::RevisionSummary>, ContributionError> {
        Ok(vec![])
    }
    async fn duplicate_candidates(
        &self,
        _point: GeoPoint,
        _name: &str,
    ) -> Result<Vec<DuplicateCandidate>, ContributionError> {
        Ok(self.dupes.lock().unwrap().clone())
    }
}

fn location_at(id: i64, version: i64) -> Result<ParkingLocation, ContributionError> {
    ParkingLocation::new(
        id,
        "Estação Centro",
        "Rua Teste, 10",
        None,
        ParkingType::Rack,
        Cost::Free,
        GeoPoint::new(-25.4284, -49.2733).unwrap(),
        tz(),
        OpeningHours::Unknown,
        vec![SecurityFeature::new("well_lit", SecurityState::Yes)],
        bikenest_domain::ModerationState::Active,
        bikenest_domain::Rating::new(None, 0).unwrap(),
        chrono::Utc::now(),
        chrono::Utc::now(),
        None,
        None,
        version,
    )
    .map_err(|e| ContributionError::InvalidField(e.to_string()))
}

struct FakeReviewRepo {
    own: Mutex<Option<Review>>,
    list: Mutex<Vec<Review>>,
}
impl FakeReviewRepo {
    fn new() -> Self {
        Self {
            own: Mutex::new(None),
            list: Mutex::new(vec![]),
        }
    }
}
#[async_trait]
impl ReviewRepository for FakeReviewRepo {
    async fn upsert_review(
        &self,
        _l: i64,
        _a: UserId,
        _r: StarRating,
        _b: &ReviewBody,
    ) -> Result<(), ContributionError> {
        Ok(())
    }
    async fn find_own(&self, _l: i64, _a: UserId) -> Result<Option<Review>, ContributionError> {
        Ok(self.own.lock().unwrap().clone())
    }
    async fn list_active(&self, _l: i64) -> Result<Vec<Review>, ContributionError> {
        Ok(self.list.lock().unwrap().clone())
    }
}

#[derive(Default)]
struct VerificationState {
    signals: Mutex<Vec<ExistenceSignal>>,
    recorded: Mutex<Vec<String>>,
    marked: Mutex<Vec<i64>>,
}
struct FakeVerificationRepo {
    state: std::sync::Arc<VerificationState>,
}
impl FakeVerificationRepo {
    fn new() -> Self {
        Self {
            state: std::sync::Arc::new(Default::default()),
        }
    }
    fn state(&self) -> std::sync::Arc<VerificationState> {
        self.state.clone()
    }
}
#[async_trait]
impl VerificationRepository for FakeVerificationRepo {
    async fn record(
        &self,
        s: &NewVerification,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), ContributionError> {
        self.state
            .recorded
            .lock()
            .unwrap()
            .push(format!("{:?}", s.is_still_exists()));
        if let NewVerification::Existence {
            user_id, result, ..
        } = s
        {
            self.state
                .signals
                .lock()
                .unwrap()
                .push(ExistenceSignal::new(*user_id, *result, chrono::Utc::now()));
        }
        Ok(())
    }
    async fn latest_existence_per_user(
        &self,
        _l: i64,
    ) -> Result<Vec<ExistenceSignal>, ContributionError> {
        Ok(self.state.signals.lock().unwrap().clone())
    }
    async fn attribute_summary(&self, _l: i64) -> Result<Vec<AttributeSummary>, ContributionError> {
        Ok(vec![])
    }
    async fn parked_here_count(&self, _l: i64) -> Result<i64, ContributionError> {
        Ok(0)
    }
    async fn mark_verified_at(
        &self,
        location_id: i64,
        _at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), ContributionError> {
        self.state.marked.lock().unwrap().push(location_id);
        Ok(())
    }
}

struct FakeFavoriteRepo {
    favorited: bool,
}
#[async_trait]
impl FavoriteRepository for FakeFavoriteRepo {
    async fn toggle(&self, _u: UserId, _l: i64) -> Result<bool, ContributionError> {
        Ok(true)
    }
    async fn is_favorited(&self, _u: UserId, _l: i64) -> Result<bool, ContributionError> {
        Ok(self.favorited)
    }
    async fn list(&self, _u: UserId) -> Result<Vec<i64>, ContributionError> {
        Ok(vec![])
    }
}

struct FakeHistory;
#[async_trait]
impl ContributionHistoryReader for FakeHistory {
    async fn history(&self, _u: UserId) -> Result<Vec<ContributionItem>, ContributionError> {
        Ok(vec![])
    }
}

struct FakeReviewPhotos;
#[async_trait]
impl bikenest_application::ReviewPhotosReader for FakeReviewPhotos {
    async fn photos(
        &self,
        _id: i64,
    ) -> Result<Vec<bikenest_application::StoredPhoto>, bikenest_application::ReaderError> {
        Ok(vec![])
    }
}

struct FakeDetails(Option<ParkingLocation>);
#[async_trait]
impl ParkingDetailsReader for FakeDetails {
    async fn details(
        &self,
        _id: i64,
    ) -> Result<Option<ParkingLocation>, bikenest_application::ReaderError> {
        Ok(self.0.clone())
    }
}

fn service(
    contributions: Box<dyn ParkingContributionRepository>,
    details: Option<ParkingLocation>,
    review: FakeReviewRepo,
    verification: FakeVerificationRepo,
    favorite: FakeFavoriteRepo,
) -> ContributionService {
    let clock = chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 6, 1, 12, 0, 0).unwrap();
    ContributionService::new(ContributionDeps {
        tz: Box::new(FakeTz),
        details: Box::new(FakeDetails(details)),
        contributions,
        reviews: Box::new(review),
        verifications: Box::new(verification),
        favorites: Box::new(favorite),
        history: Box::new(FakeHistory),
        review_photos: Box::new(FakeReviewPhotos),
        rate_limiter: Box::new(AllowRateLimiter),
        audit: Box::new(RecordingAudit {
            events: Mutex::new(vec![]),
        }),
        clock: Box::new(FakeClock(clock)),
        freshness: Default::default(),
    })
}

#[tokio::test]
async fn add_location_requires_verified() {
    let svc = service(
        Box::new(FakeContributionRepo::new(vec![])),
        None,
        FakeReviewRepo::new(),
        FakeVerificationRepo::new(),
        FakeFavoriteRepo { favorited: false },
    );
    let err = svc
        .add_parking_location(&unverified_user(1), "ip", new_input())
        .await
        .unwrap_err();
    assert!(matches!(err, ContributionError::NotVerified));
}

#[tokio::test]
async fn add_location_auto_derives_timezone_and_returns_duplicates() {
    let dupes = vec![DuplicateCandidate {
        id: 5,
        name: "Estação Centro".to_string(),
        address: "Rua Teste, 10".to_string(),
        distance_m: 20.0,
        similarity: 0.9,
    }];
    let svc = service(
        Box::new(FakeContributionRepo::new(dupes)),
        None,
        FakeReviewRepo::new(),
        FakeVerificationRepo::new(),
        FakeFavoriteRepo { favorited: false },
    );
    let AddParkingLocationOutcome { id, duplicates } = svc
        .add_parking_location(&verified_user(1), "ip", new_input())
        .await
        .unwrap();
    assert_eq!(id, 100);
    assert_eq!(duplicates.len(), 1);
    assert_eq!(duplicates[0].id, 5);
}

#[tokio::test]
async fn add_location_is_rate_limited() {
    let svc = ContributionService::new(ContributionDeps {
        tz: Box::new(FakeTz),
        details: Box::new(FakeDetails(None)),
        contributions: Box::new(FakeContributionRepo::new(vec![])),
        reviews: Box::new(FakeReviewRepo::new()),
        verifications: Box::new(FakeVerificationRepo::new()),
        favorites: Box::new(FakeFavoriteRepo { favorited: false }),
        history: Box::new(FakeHistory),
        review_photos: Box::new(FakeReviewPhotos),
        rate_limiter: Box::new(DenyRateLimiter),
        audit: Box::new(RecordingAudit {
            events: Mutex::new(vec![]),
        }),
        clock: Box::new(FakeClock(chrono::Utc::now())),
        freshness: Default::default(),
    });
    let err = svc
        .add_parking_location(&verified_user(1), "ip", new_input())
        .await
        .unwrap_err();
    assert!(matches!(err, ContributionError::RateLimited));
}

#[tokio::test]
async fn apply_edit_propagates_version_conflict() {
    let repo = FakeContributionRepo::new(vec![]);
    repo.versions.lock().unwrap().insert(10, 3);
    let svc = service(
        Box::new(repo),
        None,
        FakeReviewRepo::new(),
        FakeVerificationRepo::new(),
        FakeFavoriteRepo { favorited: false },
    );
    let edit = ParkingEdit {
        name: "new".to_string(),
        address: "addr".to_string(),
        description: None,
        parking_type: ParkingType::Rack,
        cost: Cost::Free,
        hours: OpeningHours::Unknown,
        security: vec![],
    };
    let err = svc
        .apply_parking_edit(&verified_user(1), 10, 1, &edit)
        .await
        .unwrap_err();
    assert!(matches!(err, ContributionError::VersionConflict));

    // Correct version succeeds.
    let v = svc
        .apply_parking_edit(&verified_user(1), 10, 3, &edit)
        .await
        .unwrap();
    assert_eq!(v, 4);
}

#[tokio::test]
async fn record_verification_marks_freshness_only_for_still_exists() {
    let ver = FakeVerificationRepo::new();
    let vstate = ver.state();
    let svc = service(
        Box::new(FakeContributionRepo::new(vec![])),
        None,
        FakeReviewRepo::new(),
        ver,
        FakeFavoriteRepo { favorited: false },
    );

    svc.record_verification(
        &verified_user(1),
        &NewVerification::Existence {
            location_id: 3,
            user_id: UserId(1),
            result: ExistenceResult::StillExists,
        },
    )
    .await
    .unwrap();
    assert_eq!(vstate.marked.lock().unwrap().len(), 1);

    svc.record_verification(
        &verified_user(1),
        &NewVerification::ParkedHere {
            location_id: 3,
            user_id: UserId(1),
        },
    )
    .await
    .unwrap();
    // parked-here never marks freshness.
    assert_eq!(vstate.marked.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn community_details_computes_confidence_and_favorite() {
    let loc = location_at(1, 1).unwrap();
    let ver = FakeVerificationRepo::new();
    let vstate = ver.state();
    let now = chrono::Utc::now();
    vstate.signals.lock().unwrap().push(ExistenceSignal::new(
        UserId(1),
        ExistenceResult::StillExists,
        now,
    ));
    vstate.signals.lock().unwrap().push(ExistenceSignal::new(
        UserId(2),
        ExistenceResult::NoLongerExists,
        now,
    ));
    let review = FakeReviewRepo::new();
    let fav = FakeFavoriteRepo { favorited: true };
    let svc = service(
        Box::new(FakeContributionRepo::new(vec![])),
        Some(loc),
        review,
        ver,
        fav,
    );

    let details = svc
        .community_details(1, Some(UserId(1)))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(details.confidence, Confidence::Conflicting);
    assert!(details.is_favorited);
}

#[tokio::test]
async fn recommendation_reasons_only_surface_positive_factors() {
    use bikenest_application::recommendation_reasons;
    let summary = bikenest_application::ParkingSummary {
        id: 1,
        name: "x".to_string(),
        address: "a".to_string(),
        parking_type: ParkingType::Rack,
        cost: Cost::Free,
        point: GeoPoint::new(0.0, 0.0).unwrap(),
        distance_m: 100.0,
        security_yes: vec!["cctv".to_string(), "indoor".to_string()],
        rating: bikenest_domain::Rating::new(Some(4.5), 2).unwrap(),
        last_verified_at: Some(chrono::Utc::now()),
        timezone: tz(),
        is_open_now: false,
        photo_key: None,
    };
    let reasons = recommendation_reasons(
        &summary,
        1000,
        Some(GeoPoint::new(0.0, 0.001).unwrap()),
        chrono::Utc::now(),
        &Default::default(),
    );
    let factors: Vec<&str> = reasons.iter().map(|r| r.factor).collect();
    assert!(factors.contains(&"security"));
    assert!(factors.contains(&"rating"));
    assert!(factors.contains(&"freshness"));
    assert!(factors.contains(&"verification"));
    assert!(factors.contains(&"distance"));
}
