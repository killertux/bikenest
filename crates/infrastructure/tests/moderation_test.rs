//! M5 moderation infrastructure tests: the report repo state machine, the
//! moderation actions (proposal apply + supersede, parking invalidate revision),
//! and the audit-log reader filter/pagination. Uses the committed-fixture
//! pattern (the repos read/write through the pool, on other connections).

use bikenest_application::{
    AuditFilter, AuditLogReader, ModerationError, ModerationRepository, NewReport,
    ProposalApplication, ReportRepository,
};
use bikenest_domain::{
    ModerationState, ReportDescription, ReportOutcome, ReportState, ReportTargetType, UserId,
};
use bikenest_infrastructure::{
    Db, SqlxAuditLogReader, SqlxModerationRepository, SqlxReportRepository,
};
use bikenest_test_support::{ParkingBuilder, UserBuilder, db_test, pool};

async fn db() -> Db {
    Db::from_pool(pool().await)
}

/// Commit a user (with the given role) so repo writes see it on other connections.
async fn committed_user(tx: &mut bikenest_test_support::TestTx, email: &str, role: &str) -> i64 {
    let user = UserBuilder::new()
        .with_email(email)
        .create(tx.executor())
        .await
        .unwrap();
    if role != "USER" {
        sqlx::query("INSERT INTO user_roles (user_id, role, granted_by) VALUES ($1, $2, NULL)")
            .bind(user.id.0)
            .bind(role)
            .execute(tx.executor())
            .await
            .unwrap();
    }
    tx.commit_fixture().await;
    user.id.0
}

#[db_test]
async fn report_repo_state_machine(tx: &mut bikenest_test_support::TestTx) {
    let reporter = committed_user(tx, "m5-infra-rep@example.com", "USER").await;
    let moderator = committed_user(tx, "m5-infra-mod@example.com", "MODERATOR").await;
    let repo = SqlxReportRepository::new(db().await);
    let report_id = repo
        .create(&NewReport {
            reporter_id: UserId(reporter),
            target_type: ReportTargetType::Parking,
            target_id: 1,
            reason: "duplicate".to_string(),
            description: ReportDescription::new("dup").expect("in-range description"),
        })
        .await
        .unwrap();

    let got = repo.get(report_id).await.unwrap().unwrap();
    assert_eq!(got.state, ReportState::Open);
    assert_eq!(got.reporter_id, Some(UserId(reporter)));

    // Claim: OPEN → UNDER_REVIEW.
    repo.claim(report_id, UserId(moderator)).await.unwrap();
    let got = repo.get(report_id).await.unwrap().unwrap();
    assert_eq!(got.state, ReportState::UnderReview);
    assert_eq!(got.claimed_by, Some(UserId(moderator)));

    // Claiming again (not OPEN) → InvalidState.
    assert!(matches!(
        repo.claim(report_id, UserId(moderator)).await,
        Err(ModerationError::InvalidState)
    ));

    // Resolve → RESOLVED.
    repo.resolve(
        report_id,
        UserId(moderator),
        "hidden",
        ReportOutcome::Resolved,
    )
    .await
    .unwrap();
    let got = repo.get(report_id).await.unwrap().unwrap();
    assert_eq!(got.state, ReportState::Resolved);
    assert_eq!(got.resolution_note.as_deref(), Some("hidden"));

    // Previous state-listed rows reflect the filter.
    let open = repo.list(Some(ReportState::Open)).await.unwrap();
    assert!(open.iter().all(|r| r.state == ReportState::Open));

    let _ = tx;
    sqlx::query("DELETE FROM report WHERE reporter_id = $1")
        .bind(reporter)
        .execute(&pool().await)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id IN ($1, $2)")
        .bind(reporter)
        .bind(moderator)
        .execute(&pool().await)
        .await
        .unwrap();
}

#[db_test]
async fn parking_invalidate_writes_moderation_revision(tx: &mut bikenest_test_support::TestTx) {
    let moderator = committed_user(tx, "m5-infra-mod2@example.com", "MODERATOR").await;
    const MARK: &str = "m5-infra-inv";
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
    let loc = ParkingBuilder::new()
        .with_name("Infra Invalidate")
        .with_fixture_tag(MARK)
        .with_version(1)
        .create(tx.executor())
        .await
        .unwrap();
    tx.commit_fixture().await;
    let id = loc.id();

    let repo = SqlxModerationRepository::new(db().await);
    repo.set_parking_state(
        id,
        &[ModerationState::Active],
        ModerationState::Invalid,
        UserId(moderator),
    )
    .await
    .unwrap();

    let (state, version): (String, i64) =
        sqlx::query_as("SELECT moderation_state, version FROM parking_location WHERE id = $1")
            .bind(id)
            .fetch_one(&pool().await)
            .await
            .unwrap();
    assert_eq!(state, "INVALID");
    assert_eq!(version, 2, "version bumped on moderation");

    let (rev_kind, rev_version): (String, i64) = sqlx::query_as(
        "SELECT change_kind, version FROM parking_revision WHERE location_id = $1 AND change_kind = 'moderation'")
        .bind(id).fetch_one(&pool().await).await.unwrap();
    assert_eq!(rev_kind, "moderation");
    assert_eq!(rev_version, 2);

    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(moderator)
        .execute(&pool().await)
        .await
        .unwrap();
}

#[db_test]
async fn proposal_approve_applies_change_supersedes_and_writes_revision(
    tx: &mut bikenest_test_support::TestTx,
) {
    let moderator = committed_user(tx, "m5-infra-prop-mod@example.com", "MODERATOR").await;
    const MARK: &str = "m5-infra-prop";
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
    let loc = ParkingBuilder::new()
        .with_name("Infra Proposal")
        .with_fixture_tag(MARK)
        .create(tx.executor())
        .await
        .unwrap();
    tx.commit_fixture().await;
    let id = loc.id();

    // Two PENDING proposals on the same location: an older existence-removal and
    // a newer one. Approving one must supersede the other.
    let (p1,): (i64,) = sqlx::query_as(
        "INSERT INTO parking_proposal (location_id, proposer_id, base_version, kind, proposed, status) \
         VALUES ($1, $2, 1, 'change_existence', '{\"existence\":\"removed\"}', 'PENDING') RETURNING id")
        .bind(id).bind(moderator).fetch_one(&pool().await).await.unwrap();
    let (p2,): (i64,) = sqlx::query_as(
        "INSERT INTO parking_proposal (location_id, proposer_id, base_version, kind, proposed, status) \
         VALUES ($1, $2, 1, 'change_existence', '{\"existence\":\"removed\"}', 'PENDING') RETURNING id")
        .bind(id).bind(moderator).fetch_one(&pool().await).await.unwrap();

    let repo = SqlxModerationRepository::new(db().await);
    repo.approve_proposal(
        p1,
        UserId(moderator),
        ProposalApplication::ChangeExistence { exists: false },
    )
    .await
    .unwrap();

    let (lstate, lversion): (String, i64) =
        sqlx::query_as("SELECT moderation_state, version FROM parking_location WHERE id = $1")
            .bind(id)
            .fetch_one(&pool().await)
            .await
            .unwrap();
    assert_eq!(lstate, "REMOVED", "approved existence-removal sets REMOVED");
    assert_eq!(lversion, 2);

    let (p1_status,): (String,) =
        sqlx::query_as("SELECT status FROM parking_proposal WHERE id = $1")
            .bind(p1)
            .fetch_one(&pool().await)
            .await
            .unwrap();
    assert_eq!(p1_status, "APPROVED");
    // The other PENDING proposal on the same location is superseded.
    let (p2_status,): (String,) =
        sqlx::query_as("SELECT status FROM parking_proposal WHERE id = $1")
            .bind(p2)
            .fetch_one(&pool().await)
            .await
            .unwrap();
    assert_eq!(p2_status, "SUPERSEDED");

    let (rev,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM parking_revision WHERE location_id = $1 AND change_kind = 'moderation' AND version = 2")
        .bind(id).fetch_one(&pool().await).await.unwrap();
    assert_eq!(rev, 1, "approval writes a moderation revision");

    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(moderator)
        .execute(&pool().await)
        .await
        .unwrap();
}

#[db_test]
async fn proposal_approve_refuses_a_stale_base_version(tx: &mut bikenest_test_support::TestTx) {
    let moderator = committed_user(tx, "m5-infra-stale-mod@example.com", "MODERATOR").await;
    const MARK: &str = "m5-infra-stale";
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
    let loc = ParkingBuilder::new()
        .with_name("Infra Stale Proposal")
        .with_fixture_tag(MARK)
        .with_version(5)
        .create(tx.executor())
        .await
        .unwrap();
    tx.commit_fixture().await;
    let id = loc.id();

    // Proposed against v3, but the location has moved on to v5.
    let (stale,): (i64,) = sqlx::query_as(
        "INSERT INTO parking_proposal (location_id, proposer_id, base_version, kind, proposed, status) \
         VALUES ($1, $2, 3, 'change_existence', '{\"existence\":\"removed\"}', 'PENDING') RETURNING id")
        .bind(id).bind(moderator).fetch_one(&pool().await).await.unwrap();

    let repo = SqlxModerationRepository::new(db().await);
    let err = repo
        .approve_proposal(
            stale,
            UserId(moderator),
            ProposalApplication::ChangeExistence { exists: false },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, ModerationError::StaleProposal),
        "expected StaleProposal, got {err:?}"
    );

    // Nothing changed: same state, same version, no revision, still PENDING.
    let (state, version): (String, i64) =
        sqlx::query_as("SELECT moderation_state, version FROM parking_location WHERE id = $1")
            .bind(id)
            .fetch_one(&pool().await)
            .await
            .unwrap();
    assert_eq!(state, "ACTIVE");
    assert_eq!(version, 5);
    let (revs,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM parking_revision WHERE location_id = $1")
            .bind(id)
            .fetch_one(&pool().await)
            .await
            .unwrap();
    assert_eq!(revs, 0, "a refused approval writes no revision");
    let (status,): (String,) = sqlx::query_as("SELECT status FROM parking_proposal WHERE id = $1")
        .bind(stale)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    assert_eq!(status, "PENDING");

    // A proposal made against the current version still applies.
    let (fresh,): (i64,) = sqlx::query_as(
        "INSERT INTO parking_proposal (location_id, proposer_id, base_version, kind, proposed, status) \
         VALUES ($1, $2, 5, 'change_existence', '{\"existence\":\"removed\"}', 'PENDING') RETURNING id")
        .bind(id).bind(moderator).fetch_one(&pool().await).await.unwrap();
    repo.approve_proposal(
        fresh,
        UserId(moderator),
        ProposalApplication::ChangeExistence { exists: false },
    )
    .await
    .unwrap();
    let (state, version): (String, i64) =
        sqlx::query_as("SELECT moderation_state, version FROM parking_location WHERE id = $1")
            .bind(id)
            .fetch_one(&pool().await)
            .await
            .unwrap();
    assert_eq!(state, "REMOVED");
    assert_eq!(version, 6);
    // Approving it superseded the stale sibling.
    let (status,): (String,) = sqlx::query_as("SELECT status FROM parking_proposal WHERE id = $1")
        .bind(stale)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    assert_eq!(status, "SUPERSEDED");

    let _ = tx;
    sqlx::query("DELETE FROM parking_location WHERE seed_key = $1")
        .bind(MARK)
        .execute(&pool().await)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(moderator)
        .execute(&pool().await)
        .await
        .unwrap();
}

#[db_test]
async fn report_dedupe_index_rejects_a_second_open_report(
    tx: &mut bikenest_test_support::TestTx,
) {
    let reporter = committed_user(tx, "m5-infra-dupe-a@example.com", "USER").await;
    let other = committed_user(tx, "m5-infra-dupe-b@example.com", "USER").await;
    let moderator = committed_user(tx, "m5-infra-dupe-mod@example.com", "MODERATOR").await;
    let repo = SqlxReportRepository::new(db().await);

    let new = |who: i64| NewReport {
        reporter_id: UserId(who),
        target_type: ReportTargetType::Parking,
        target_id: 424_242,
        reason: "duplicate".to_string(),
        description: ReportDescription::new("dup").expect("in-range description"),
    };

    let first = repo.create(&new(reporter)).await.unwrap();

    // Same reporter, same target, still open → the partial unique index fires
    // and `db_err` classifies it as a Conflict (the service maps it onward).
    let err = repo.create(&new(reporter)).await.unwrap_err();
    assert!(
        matches!(err, ModerationError::Conflict),
        "expected Conflict, got {err:?}"
    );

    // A different reporter on the same target is a distinct signal.
    let second_reporter = repo.create(&new(other)).await.unwrap();

    // Once the first is resolved, the same reporter may report it again.
    repo.claim(first, UserId(moderator)).await.unwrap();
    repo.resolve(first, UserId(moderator), "done", ReportOutcome::Resolved)
        .await
        .unwrap();
    let third = repo.create(&new(reporter)).await.unwrap();
    assert_ne!(third, first);

    let _ = tx;
    let _ = second_reporter;
    sqlx::query("DELETE FROM report WHERE reporter_id = ANY($1)")
        .bind(vec![reporter, other])
        .execute(&pool().await)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = ANY($1)")
        .bind(vec![reporter, other, moderator])
        .execute(&pool().await)
        .await
        .unwrap();
}

#[db_test]
async fn audit_reader_filters_and_paginates(tx: &mut bikenest_test_support::TestTx) {
    let actor = committed_user(tx, "m5-infra-audit@example.com", "USER").await;
    let reader = SqlxAuditLogReader::new(db().await);
    // Insert a batch of audit events, then filter by action + keyset paginate.
    for i in 0..5 {
        sqlx::query!(
            "INSERT INTO audit_events (actor_user_id, action, target_type, target_id, result, metadata) \
             VALUES ($1, $2, $3, $4, 'success', '{}'::jsonb)",
            actor,
            if i % 2 == 0 { "mod.foo" } else { "mod.bar" },
            "report",
            i.to_string(),
        )
        .execute(&pool().await)
        .await
        .unwrap();
    }
    let page = reader
        .list(AuditFilter {
            action: Some("mod.foo".to_string()),
            actor_id: Some(UserId(actor)),
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(page.items.len() >= 3, "filter by action, actor");
    assert!(page.items.iter().all(|e| e.event.action == "mod.foo"));

    // Keyset pagination returns a next_cursor and a strictly smaller id set.
    let first = reader
        .list(AuditFilter {
            limit: 2,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(first.items.len(), 2);
    assert!(first.next_cursor.is_some());
    let second = reader
        .list(AuditFilter {
            cursor: first.next_cursor,
            limit: 2,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(
        second.items.iter().all(|e| e.id < first.items[0].id),
        "keyset id DESC"
    );

    let _ = tx;
    sqlx::query("DELETE FROM audit_events WHERE actor_user_id = $1")
        .bind(actor)
        .execute(&pool().await)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(actor)
        .execute(&pool().await)
        .await
        .unwrap();
}

// Note: these tests require the migration applied (0010/0011). The suite's
// `sqlx::migrate!` runs them on startup.
