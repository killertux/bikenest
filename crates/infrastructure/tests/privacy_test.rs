//! M6 privacy infrastructure tests (plans/m6-privacy.md §12): the export
//! payload (no secrets), the single-use download token, the anonymize-in-place
//! transaction, the retention purge statements, and the policy reader.
//!
//! Uses the committed-fixture pattern: fixtures are inserted through the test
//! transaction and committed with `commit_fixture` so the repos (which read /
//! write through shared pool connections) can see them.

use bikenest_application::{
    AnonymizationRepository, ExportAccount, ExportPayload, ExportRepository, NewExport,
    PolicyReader, PrivacyError, RetentionRepository,
};
use bikenest_domain::{PolicyKind, RetentionPolicy, UserId};
use bikenest_infrastructure::{
    Db, SqlxAnonymizationRepository, SqlxExportRepository, SqlxPolicyReader,
    SqlxRetentionRepository,
};
use bikenest_test_support::{ParkingBuilder, UserBuilder, db_test, pool};
use chrono::{DateTime, Duration, Utc};

async fn db() -> Db {
    Db::from_pool(pool().await)
}

fn empty_payload(user_id: i64) -> ExportPayload {
    ExportPayload::new(
        ExportAccount {
            user_id,
            email: "a@example.com".to_string(),
            display_name: None,
            account_state: "ACTIVE".to_string(),
            email_verified_at: None,
            created_at: Utc::now(),
            roles: vec![],
        },
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        Utc::now(),
    )
}

fn af(t: impl AsRef<str>) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(t.as_ref())
        .unwrap()
        .with_timezone(&Utc)
}

#[db_test]
async fn export_payload_excludes_credential_hash(tx: &mut bikenest_test_support::TestTx) {
    let user = UserBuilder::new().with_email("m6exp@example.com").create(tx.executor()).await.unwrap();
    let uid = user.id.0;
    sqlx::query(
        "INSERT INTO authentication_identities (user_id, provider, provider_subject, credential_hash) \
         VALUES ($1, 'password', $2, 'supersecret-hash')",
    )
    .bind(uid)
    .bind("m6exp@example.com")
    .execute(tx.executor())
    .await
    .unwrap();
    tx.commit_fixture().await;

    let repo = SqlxExportRepository::new(db().await);
    let payload = repo.assemble_payload(UserId(uid)).await.unwrap();
    assert_eq!(payload.schema_version, 1);
    assert_eq!(payload.authentication.len(), 1);
    // credential_hash is never selected into the payload.
    let json = serde_json::to_string(&payload).unwrap();
    assert!(!json.contains("supersecret-hash"));

    // Clean up the committed fixture user.
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(uid)
        .execute(&pool().await)
        .await
        .unwrap();
}

#[db_test]
async fn export_consume_download_is_single_use_and_distinguishes_errors(
    tx: &mut bikenest_test_support::TestTx,
) {
    let user = UserBuilder::new().with_email("m6exp2@example.com").create(tx.executor()).await.unwrap();
    let uid = user.id.0;
    tx.commit_fixture().await;

    let repo = SqlxExportRepository::new(db().await);
    let token = [7u8; 32];
    let now = Utc::now();
    let id = repo
        .create(&NewExport {
            user_id: UserId(uid),
            token,
            payload: empty_payload(uid),
            expires_at: now + Duration::hours(24),
        })
        .await
        .unwrap();

    // First download succeeds.
    repo.consume_download(id, &token, now).await.unwrap();

    // Second download = AlreadyDownloaded.
    let e2 = repo.consume_download(id, &token, now).await.unwrap_err();
    assert!(matches!(e2, PrivacyError::AlreadyDownloaded));

    // A different token (not yet consumed) reports InvalidToken.
    let id2 = repo
        .create(&NewExport {
            user_id: UserId(uid),
            token: [8u8; 32],
            payload: empty_payload(uid),
            expires_at: now + Duration::hours(24),
        })
        .await
        .unwrap();
    let e3 = repo.consume_download(id2, &[9u8; 32], now).await.unwrap_err();
    assert!(matches!(e3, PrivacyError::InvalidToken));

    // An expired export reports Expired.
    let id3 = repo
        .create(&NewExport {
            user_id: UserId(uid),
            token: [10u8; 32],
            payload: empty_payload(uid),
            expires_at: now - Duration::hours(1),
        })
        .await
        .unwrap();
    let e4 = repo.consume_download(id3, &[10u8; 32], now).await.unwrap_err();
    assert!(matches!(e4, PrivacyError::Expired));

    // Clean up the committed fixture user (cascades to exports).
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(uid)
        .execute(&pool().await)
        .await
        .unwrap();
}

#[db_test]
async fn anonymize_scrubs_identity_and_nulls_attribution(tx: &mut bikenest_test_support::TestTx) {
    // A location to hang content off of.
    let loc = ParkingBuilder::new().with_name("M6 Anon").create(tx.executor()).await.unwrap();
    let loc_id = loc.id();
    let user = UserBuilder::new().with_email("m6anon@example.com").with_name("Ada").create(tx.executor()).await.unwrap();
    let uid = user.id.0;

    // Identity + session (private, deleted).
    sqlx::query(
        "INSERT INTO authentication_identities (user_id, provider, provider_subject, credential_hash) \
         VALUES ($1, 'password', $2, 'hash')",
    )
    .bind(uid).bind("m6anon@example.com").execute(tx.executor()).await.unwrap();
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, csrf_token, created_at, last_seen_at, expires_at) \
         VALUES ('tok', $1, 'csrf', now(), now(), now() + interval '30 days')",
    )
    .bind(uid).execute(tx.executor()).await.unwrap();
    sqlx::query("INSERT INTO favorite (user_id, location_id, created_at) VALUES ($1, $2, now())")
        .bind(uid).bind(loc_id).execute(tx.executor()).await.unwrap();

    // Parked-here (private, deleted).
    sqlx::query(
        "INSERT INTO verification (location_id, user_id, kind, result, created_at, expires_at) \
         VALUES ($1, $2, 'parked_here', 'still_exists', now(), now() + interval '90 days')",
    )
    .bind(loc_id).bind(uid).execute(tx.executor()).await.unwrap();
    // Existence verification (community content, retained but unattributed).
    sqlx::query(
        "INSERT INTO verification (location_id, user_id, kind, result, created_at) \
         VALUES ($1, $2, 'existence', 'still_exists', now())",
    )
    .bind(loc_id).bind(uid).execute(tx.executor()).await.unwrap();
    // Review (retained, author NULL).
    sqlx::query(
        "INSERT INTO review (location_id, author_id, rating, body, moderation_state, created_at, updated_at) \
         VALUES ($1, $2, 5, 'Great', 'ACTIVE', now(), now())",
    )
    .bind(loc_id).bind(uid).execute(tx.executor()).await.unwrap();
    // Proposal (retained, proposer NULL).
    sqlx::query(
        "INSERT INTO parking_proposal (location_id, proposer_id, base_version, kind, proposed, status, created_at) \
         VALUES ($1, $2, 1, 'change_existence', '{\"existence\":\"exists\"}', 'PENDING', now())",
    )
    .bind(loc_id).bind(uid).execute(tx.executor()).await.unwrap();
    // Report (retained, reporter NULL).
    sqlx::query(
        "INSERT INTO report (reporter_id, target_type, target_id, reason, state, created_at, updated_at) \
         VALUES ($1, 'parking', $2, 'spam', 'OPEN', now(), now())",
    )
    .bind(uid).bind(loc_id).execute(tx.executor()).await.unwrap();
    // Photos (retained, uploader NULL).
    sqlx::query(
        "INSERT INTO parking_photo (location_id, storage_key, content_type, moderation_state, created_at, uploader_id) \
         VALUES ($1, 'seed/a.jpg', 'image/jpeg', 'APPROVED', now(), $2)",
    )
    .bind(loc_id).bind(uid).execute(tx.executor()).await.unwrap();
    // A privacy request (kept, user_id nulled).
    sqlx::query(
        "INSERT INTO privacy_request (user_id, kind, state, details) VALUES ($1, 'deletion', 'OPEN', '{}')",
    )
    .bind(uid).execute(tx.executor()).await.unwrap();

    tx.commit_fixture().await;

    let now = Utc::now();
    let repo = SqlxAnonymizationRepository::new(db().await);
    let report = repo.anonymize(UserId(uid), now).await.unwrap();

    let pool = pool().await;
    // user scrubbed.
    let (email, state, deleted_at) = sqlx::query_as::<_, (String, String, Option<DateTime<Utc>>)>(
        "SELECT email, account_state, deleted_at FROM users WHERE id = $1",
    )
    .bind(uid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(email, format!("deleted+{uid}@bikenest.invalid"));
    assert_eq!(state, "DELETED");
    assert!(deleted_at.is_some());

    // private activity gone.
    let identities: i64 = sqlx::query_scalar("SELECT count(*) FROM authentication_identities WHERE user_id = $1")
        .bind(uid).fetch_one(&pool).await.unwrap();
    assert_eq!(identities, 0);
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id = $1")
        .bind(uid).fetch_one(&pool).await.unwrap();
    assert_eq!(sessions, 0);
    let favs: i64 = sqlx::query_scalar("SELECT count(*) FROM favorite WHERE user_id = $1")
        .bind(uid).fetch_one(&pool).await.unwrap();
    assert_eq!(favs, 0);
    let parked: i64 = sqlx::query_scalar("SELECT count(*) FROM verification WHERE user_id = $1")
        .bind(uid).fetch_one(&pool).await.unwrap();
    assert_eq!(parked, 0);

    // community content retained + unattributed.
    let review_author: Option<i64> = sqlx::query_scalar("SELECT author_id FROM review WHERE location_id = $1")
        .bind(loc_id).fetch_one(&pool).await.unwrap();
    assert!(review_author.is_none());
    let existence_user: Option<i64> = sqlx::query_scalar(
        "SELECT user_id FROM verification WHERE location_id = $1 AND kind = 'existence'",
    )
    .bind(loc_id).fetch_one(&pool).await.unwrap();
    assert!(existence_user.is_none());
    let prop: Option<i64> = sqlx::query_scalar("SELECT proposer_id FROM parking_proposal WHERE location_id = $1")
        .bind(loc_id).fetch_one(&pool).await.unwrap();
    assert!(prop.is_none());
    let rep: Option<i64> = sqlx::query_scalar("SELECT reporter_id FROM report WHERE target_id = $1 AND target_type = 'parking'")
        .bind(loc_id).fetch_one(&pool).await.unwrap();
    assert!(rep.is_none());
    let photo: Option<i64> = sqlx::query_scalar("SELECT uploader_id FROM parking_photo WHERE location_id = $1")
        .bind(loc_id).fetch_one(&pool).await.unwrap();
    assert!(photo.is_none());
    let req_user: Option<i64> = sqlx::query_scalar("SELECT user_id FROM privacy_request WHERE details = '{}'")
        .fetch_one(&pool).await.unwrap();
    assert!(req_user.is_none());

    // Report counts.
    assert_eq!(report.identities, 1);
    assert_eq!(report.sessions, 1);
    assert_eq!(report.favorites, 1);
    assert_eq!(report.parked_here, 1);
    assert_eq!(report.reviews_anonymized, 1);
    assert_eq!(report.verifications_anonymized, 1);
    assert_eq!(report.proposals_anonymized, 1);
    assert_eq!(report.reports_anonymized, 1);
    assert_eq!(report.parking_photos_anonymized, 1);
    assert_eq!(report.privacy_requests_anonymized, 1);

    // The anonymized shell is the only remaining row referencing the user; remove it.
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
}

#[db_test]
async fn retention_purges_only_expired(tx: &mut bikenest_test_support::TestTx) {
    let user = UserBuilder::new().with_email("m6ret@example.com").create(tx.executor()).await.unwrap();
    let uid = user.id.0;
    let now = Utc::now();
    // One expired + one valid password-reset token.
    sqlx::query(
        "INSERT INTO password_reset_tokens (token_hash, user_id, created_at, expires_at) VALUES ('expired', $1, now(), $2)",
    )
    .bind(uid).bind(now - Duration::hours(2)).execute(tx.executor()).await.unwrap();
    sqlx::query(
        "INSERT INTO password_reset_tokens (token_hash, user_id, created_at, expires_at) VALUES ('valid', $1, now(), $2)",
    )
    .bind(uid).bind(now + Duration::hours(2)).execute(tx.executor()).await.unwrap();
    tx.commit_fixture().await;

    let repo = SqlxRetentionRepository::new(db().await, RetentionPolicy::default(), Box::new(db_storage()), "media".into());
    let n = repo.purge_expired_password_reset_tokens(now).await.unwrap();
    assert_eq!(n, 1);
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM password_reset_tokens WHERE user_id = $1")
        .bind(uid).fetch_one(&pool().await).await.unwrap();
    assert_eq!(remaining, 1);

    // Clean up the committed fixture user (cascades to password_reset_tokens).
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(uid)
        .execute(&pool().await)
        .await
        .unwrap();
}

#[db_test]
async fn policy_reader_current_and_history(tx: &mut bikenest_test_support::TestTx) {
    let reader = SqlxPolicyReader::new(db().await);
    let old = "m6-test-old";
    let new = "m6-test-new";

    // Insert an old + a new privacy version with far-future effective dates so
    // the reader's `current` (newest effective, not superseded) is deterministic
    // regardless of any rows already in the shared dev DB.
    sqlx::query(
        "INSERT INTO policy_version (kind, version, effective_at, content) VALUES ('privacy', $1, $2, 'old')",
    )
    .bind(old)
    .bind(af("2030-01-01T00:00:00Z"))
    .execute(tx.executor())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO policy_version (kind, version, effective_at, content) VALUES ('privacy', $1, $2, 'new')",
    )
    .bind(new)
    .bind(af("2031-01-01T00:00:00Z"))
    .execute(tx.executor())
    .await
    .unwrap();
    tx.commit_fixture().await;

    let current = reader.current(PolicyKind::Privacy).await.unwrap().unwrap();
    assert_eq!(current.version, "m6-test-new");
    assert!(current.superseded_at.is_none());

    let history = reader.history(PolicyKind::Privacy).await.unwrap();
    assert!(history.iter().any(|d| d.version == old));
    assert!(history.iter().any(|d| d.version == new));
    // Newest first.
    assert_eq!(history[0].version, "m6-test-new");

    // Clean up the fixture rows (they are committed, so they persist).
    let pool = pool().await;
    sqlx::query("DELETE FROM policy_version WHERE version = $1 OR version = $2")
        .bind(old)
        .bind(new)
        .execute(&pool)
        .await
        .unwrap();
}

// A local-disk ObjectStorage for the retention repo (best-effort media sweep).
fn db_storage() -> bikenest_infrastructure::LocalDiskStorage {
    bikenest_infrastructure::LocalDiskStorage::new(
        std::env::temp_dir().join("bikenest-privacy-retention-media"),
        b"test-secret".to_vec(),
    )
}
