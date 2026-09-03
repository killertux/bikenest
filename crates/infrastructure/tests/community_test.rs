//! Real-PostgreSQL integration tests for the M3 community repositories.
//!
//! The write repos create their own transactions on pool connections (they
//! commit to the shared DB), so these tests use the **committed-fixture**
//! pattern: they create a fixture user + location, run the repo, assert against
//! the repos (which read committed rows), then clean up both. Each test gets a
//! unique user email so parallel tests never collide.

use bikenest_application::{
    ContributionError, ContributionHistoryReader, FavoriteRepository, NewParkingLocation,
    NewProposal, NewVerification, ParkingContributionRepository, ParkingEdit, ReviewRepository,
    VerificationRepository,
};
use bikenest_domain::{
    AttributeResult, Cost, CurrencyCode, ExistenceResult, GeoPoint, Money, OpeningHours,
    ParkingType, PricingUnit, ReviewBody, SecurityFeature, SecurityState, StarRating, UserId,
};
use bikenest_infrastructure::{
    Db, SqlxContributionHistoryReader, SqlxFavoriteRepository, SqlxParkingContributionRepository,
    SqlxReviewRepository, SqlxVerificationRepository,
};
use bikenest_test_support::{UserBuilder, db_test, pool};

async fn db() -> Db {
    Db::from_pool(pool().await)
}

/// A unique, clean user per test. Uses the commit-fixture + explicit cleanup so
/// the write repos (running on pool connections) can honor the FK.
async fn fresh_user(tx: &mut bikenest_test_support::TestTx, email: &str) -> UserId {
    // Clean any leftover from a prior run of the same name.
    sqlx::query(
        "DELETE FROM parking_location WHERE creator_id = (SELECT id FROM users WHERE email = $1)",
    )
    .bind(email)
    .execute(&pool().await)
    .await
    .unwrap();
    sqlx::query("DELETE FROM users WHERE email = $1")
        .bind(email)
        .execute(&pool().await)
        .await
        .unwrap();
    let user = UserBuilder::new()
        .with_email(email)
        .create(tx.executor())
        .await
        .unwrap();
    tx.commit_fixture().await;
    user.id
}

async fn cleanup_user(email: &str) {
    sqlx::query(
        "DELETE FROM parking_location WHERE creator_id = (SELECT id FROM users WHERE email = $1)",
    )
    .bind(email)
    .execute(&pool().await)
    .await
    .unwrap();
    sqlx::query("DELETE FROM users WHERE email = $1")
        .bind(email)
        .execute(&pool().await)
        .await
        .unwrap();
}

fn new_location() -> NewParkingLocation {
    NewParkingLocation {
        name: "Estação Centro".to_string(),
        address: "Rua Teste, 10".to_string(),
        description: None,
        parking_type: ParkingType::Rack,
        cost: Cost::Free,
        point: GeoPoint::new(-25.4284, -49.2733).unwrap(),
        timezone: Some("America/Sao_Paulo".parse().unwrap()),
        hours: OpeningHours::Unknown,
        security: vec![SecurityFeature::new("well_lit", SecurityState::Yes)],
    }
}

#[db_test]
async fn create_writes_location_revision_and_reads_back(tx: &mut bikenest_test_support::TestTx) {
    let email = "c-create@test.dev";
    let user = fresh_user(tx, email).await;
    let repo = SqlxParkingContributionRepository::new(db().await);

    let id = repo
        .create(&new_location(), user, chrono::Utc::now())
        .await
        .unwrap();
    assert!(id > 0);

    let current = repo.get_for_edit(id).await.unwrap().unwrap();
    assert_eq!(current.version(), 1);
    assert_eq!(current.name(), "Estação Centro");

    let history = repo.revision_history(id).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].version, 1);
    use bikenest_domain::ChangeKind;
    assert_eq!(history[0].change_kind, ChangeKind::Create);

    cleanup_user(email).await;
}

#[db_test]
async fn optimistic_edit_wins_only_on_expected_version(tx: &mut bikenest_test_support::TestTx) {
    let email = "c-edit@test.dev";
    let user = fresh_user(tx, email).await;
    let repo = SqlxParkingContributionRepository::new(db().await);
    let id = repo
        .create(&new_location(), user, chrono::Utc::now())
        .await
        .unwrap();

    let edit = ParkingEdit {
        name: "Estação Centro (renovada)".to_string(),
        address: "Av. Nova, 20".to_string(),
        description: Some("renovated".to_string()),
        parking_type: ParkingType::Secured,
        cost: Cost::Paid {
            price: Some(Money::new(
                200,
                CurrencyCode::parse("BRL").unwrap(),
                PricingUnit::Hour,
            )),
        },
        hours: OpeningHours::Unknown,
        security: vec![SecurityFeature::new("cctv", SecurityState::Yes)],
    };

    // Correct version → succeeds and bumps to 2.
    let new_version = repo
        .apply_edit(id, 1, &edit, user, chrono::Utc::now())
        .await
        .unwrap();
    assert_eq!(new_version, 2);
    let current = repo.get_for_edit(id).await.unwrap().unwrap();
    assert_eq!(current.version(), 2);

    // Stale version → conflict, no change.
    let err = repo
        .apply_edit(id, 1, &edit, user, chrono::Utc::now())
        .await
        .unwrap_err();
    assert!(matches!(err, ContributionError::VersionConflict));
    let current = repo.get_for_edit(id).await.unwrap().unwrap();
    assert_eq!(current.version(), 2);

    // History now has two rows (create + edit).
    let history = repo.revision_history(id).await.unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].version, 2);
    assert_eq!(history[1].version, 1);

    cleanup_user(email).await;
}

#[db_test]
async fn proposal_is_pending_with_no_live_change(tx: &mut bikenest_test_support::TestTx) {
    let email = "c-proposal@test.dev";
    let user = fresh_user(tx, email).await;
    let repo = SqlxParkingContributionRepository::new(db().await);
    let id = repo
        .create(&new_location(), user, chrono::Utc::now())
        .await
        .unwrap();

    let p = NewProposal {
        location_id: id,
        proposer_id: user,
        base_version: 1,
        kind: bikenest_domain::ProposalKind::MoveLocation,
        proposed: serde_json::json!({"lat": -25.0, "lon": -49.0, "reason": "moved"}),
    };
    let pid = repo.create_proposal(&p).await.unwrap();
    assert!(pid > 0);

    // No live change; location still at version 1.
    let current = repo.get_for_edit(id).await.unwrap().unwrap();
    assert_eq!(current.version(), 1);

    let row: (String,) = sqlx::query_as("SELECT status FROM parking_proposal WHERE id = $1")
        .bind(pid)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    assert_eq!(row.0, "PENDING");

    cleanup_user(email).await;
}

#[db_test]
async fn review_upsert_recomputes_rating_and_appends_history(
    tx: &mut bikenest_test_support::TestTx,
) {
    let email = "c-review@test.dev";
    let user = fresh_user(tx, email).await;
    let repo = SqlxParkingContributionRepository::new(db().await);
    let id = repo
        .create(&new_location(), user, chrono::Utc::now())
        .await
        .unwrap();

    let reviews = SqlxReviewRepository::new(db().await);
    let body = ReviewBody::new("gostei muito da estrutura").unwrap();

    // Two different authors.
    let user2 = fresh_user(tx, "c-review-2@test.dev").await;

    reviews
        .upsert_review(id, user, StarRating::new(4).unwrap(), &body)
        .await
        .unwrap();
    reviews
        .upsert_review(id, user2, StarRating::new(5).unwrap(), &body)
        .await
        .unwrap();

    let active = reviews.list_active(id).await.unwrap();
    assert_eq!(active.len(), 2);

    // Aggregate recomputed == direct COUNT/AVG.
    let db = db().await;
    let row: (Option<f64>, i32) = sqlx::query_as(
        "SELECT rating_avg::float8, rating_count FROM parking_location WHERE id = $1",
    )
    .bind(id)
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(row.1, 2);
    assert!((row.0.unwrap() - 4.5).abs() < 0.001, "avg = {:?}", row.0);

    // Editing author `user` appends a revision but keeps one row.
    let body2 = ReviewBody::new("updated").unwrap();
    let existed = reviews.find_own(id, user).await.unwrap().is_some();
    assert!(existed);
    reviews
        .upsert_review(id, user, StarRating::new(2).unwrap(), &body2)
        .await
        .unwrap();
    let active = reviews.list_active(id).await.unwrap();
    assert_eq!(active.len(), 2);
    let own = reviews.find_own(id, user).await.unwrap().unwrap();
    assert_eq!(own.rating.value(), 2);
    let rev_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM review_revision WHERE review_id = $1")
            .bind(own.id)
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(rev_count.0, 2);

    cleanup_user(email).await;
    cleanup_user("c-review-2@test.dev").await;
}

#[db_test]
async fn verification_still_exists_sets_last_verified_at(tx: &mut bikenest_test_support::TestTx) {
    let email = "c-verify@test.dev";
    let user = fresh_user(tx, email).await;
    let repo = SqlxParkingContributionRepository::new(db().await);
    let id = repo
        .create(&new_location(), user, chrono::Utc::now())
        .await
        .unwrap();

    let ver = SqlxVerificationRepository::new(db().await);

    ver.record(
        &NewVerification::Existence {
            location_id: id,
            user_id: user,
            result: ExistenceResult::StillExists,
        },
        chrono::Utc::now(),
    )
    .await
    .unwrap();
    // The *service* calls mark_verified_at for a positive existence signal;
    // the repo exposes it as a separate method. Test it here.
    ver.mark_verified_at(id, chrono::Utc::now()).await.unwrap();
    let verified: (bool,) =
        sqlx::query_as("SELECT last_verified_at IS NOT NULL FROM parking_location WHERE id = $1")
            .bind(id)
            .fetch_one(db().await.pool())
            .await
            .unwrap();
    assert!(verified.0);

    // A later no_longer_exists from another user → two latest-per-user signals.
    let user2 = fresh_user(tx, "c-verify-2@test.dev").await;
    ver.record(
        &NewVerification::Existence {
            location_id: id,
            user_id: user2,
            result: ExistenceResult::NoLongerExists,
        },
        chrono::Utc::now(),
    )
    .await
    .unwrap();
    let signals = ver.latest_existence_per_user(id).await.unwrap();
    assert_eq!(signals.len(), 2);

    // Attribute summary tallies per code.
    ver.record(
        &NewVerification::Attribute {
            location_id: id,
            user_id: user,
            code: "name".to_string(),
            result: AttributeResult::Incorrect,
        },
        chrono::Utc::now(),
    )
    .await
    .unwrap();
    let summary = ver.attribute_summary(id).await.unwrap();
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].incorrect, 1);

    // Parked-here count + expiry set.
    ver.record(
        &NewVerification::ParkedHere {
            location_id: id,
            user_id: user,
        },
        chrono::Utc::now(),
    )
    .await
    .unwrap();
    let count = ver.parked_here_count(id).await.unwrap();
    assert_eq!(count, 1);
    let exp: (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT expires_at FROM verification WHERE kind = 'parked_here' LIMIT 1")
            .fetch_one(db().await.pool())
            .await
            .unwrap();
    assert!(exp.0.is_some());

    cleanup_user(email).await;
    cleanup_user("c-verify-2@test.dev").await;
}

#[db_test]
async fn favorite_toggle_is_idempotent(tx: &mut bikenest_test_support::TestTx) {
    let email = "c-fav@test.dev";
    let user = fresh_user(tx, email).await;
    let repo = SqlxParkingContributionRepository::new(db().await);
    let id = repo
        .create(&new_location(), user, chrono::Utc::now())
        .await
        .unwrap();

    let fav = SqlxFavoriteRepository::new(db().await);
    assert!(fav.toggle(user, id).await.unwrap()); // now favorited
    assert!(fav.is_favorited(user, id).await.unwrap());
    assert!(!fav.toggle(user, id).await.unwrap()); // now unfavorited
    assert!(!fav.is_favorited(user, id).await.unwrap());
    assert_eq!(fav.list(user).await.unwrap().len(), 0);

    fav.toggle(user, id).await.unwrap();
    assert_eq!(fav.list(user).await.unwrap(), vec![id]);

    cleanup_user(email).await;
}

#[db_test]
async fn history_reads_contributions_across_sources(tx: &mut bikenest_test_support::TestTx) {
    let email = "c-history@test.dev";
    let user = fresh_user(tx, email).await;
    let repo = SqlxParkingContributionRepository::new(db().await);
    let id = repo
        .create(&new_location(), user, chrono::Utc::now())
        .await
        .unwrap();

    let favorite = SqlxFavoriteRepository::new(db().await);
    favorite.toggle(user, id).await.unwrap();

    let history = SqlxContributionHistoryReader::new(db().await);
    let items = history.history(user).await.unwrap();
    let kinds: Vec<&str> = items.iter().map(|i| i.kind.as_str()).collect();
    assert!(kinds.contains(&"added"));
    assert!(kinds.contains(&"favorited"));
    assert!(items.len() >= 2);

    cleanup_user(email).await;
}
