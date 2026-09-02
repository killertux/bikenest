//! Database-backed auth integration tests against real PostgreSQL (§49).
//!
//! The auth repo/store write through the pool (they take `Db`, not the test
//! transaction), so each test seeds a user with a unique email marker, asserts
//! against readers, deletes the user (cascading to identities/sessions/tokens/
//! roles), and never leaks rows.

use bikenest_application::{
    AccountRepository, AuditEvent, AuditLog, SessionStore, TokenStore,
};
use bikenest_domain::{
    AccountState, AuthenticationProvider, CsrfToken, Role, SessionId, UserEmail, VerificationToken,
};
use bikenest_infrastructure::{
    Db, SqlxAccountRepository, SqlxAuditLog, SqlxSessionStore, SqlxTokenStore,
};
use bikenest_test_support::{db_test, pool};
use chrono::{Duration, Utc};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn unique_email(label: &str) -> String {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{label}-{}-{n}@bikenest.test", std::process::id())
}

fn marker_email(label: &str) -> String {
    unique_email(label)
}

async fn cleanup_user(email: &str) {
    sqlx::query("DELETE FROM users WHERE email = $1")
        .bind(email)
        .execute(&pool().await)
        .await
        .unwrap();
}

#[db_test]
async fn account_repo_round_trip(_tx: &mut bikenest_test_support::TestTx) {
    let db = Db::from_pool(pool().await);
    let repo = SqlxAccountRepository::new(db);
    let email = marker_email("repo");
    cleanup_user(&email).await;
    let eu = UserEmail::parse(&email).unwrap();

    let id = repo
        .create(bikenest_application::NewAccount {
            email: &eu,
            display_name: Some("Ada"),
            password_hash: "$argon2id$test",
            state: AccountState::Active,
        })
        .await
        .unwrap();

    let found = repo.find_by_email(&eu).await.unwrap().unwrap();
    assert_eq!(found.id, id);
    assert_eq!(found.account_state, AccountState::Active);
    assert!(found.has_role(Role::User), "USER is the implicit baseline");

    // Password identity carries the hash; subject == lowercased email.
    let idrec = repo
        .find_identity(AuthenticationProvider::Password, email.as_str())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(idrec.user_id, id);
    assert_eq!(idrec.credential_hash.as_deref(), Some("$argon2id$test"));

    // Role grant / revoke.
    repo.grant_role(id, Role::Moderator, id).await.unwrap();
    let roles = repo.roles(id).await.unwrap();
    assert!(roles.contains(&Role::Moderator));
    assert!(repo.revoke_role(id, Role::Moderator).await.unwrap());

    // Verify + list.
    repo.mark_email_verified(id, Utc::now()).await.unwrap();
    let found2 = repo.find_by_id(id).await.unwrap().unwrap();
    assert!(found2.is_verified());
    let all = repo.list_users().await.unwrap();
    assert!(all.iter().any(|u| u.id == id));

    cleanup_user(&email).await;
}

#[db_test]
async fn update_canonical_email_keeps_identity_in_sync(_tx: &mut bikenest_test_support::TestTx) {
    let db = Db::from_pool(pool().await);
    let repo = SqlxAccountRepository::new(db);
    let email = marker_email("sync");
    cleanup_user(&email).await;
    let eu = UserEmail::parse(&email).unwrap();

    let id = repo
        .create(bikenest_application::NewAccount {
            email: &eu,
            display_name: None,
            password_hash: "h",
            state: AccountState::Active,
        })
        .await
        .unwrap();

    let new_email = UserEmail::parse("renamed@bikenest.test").unwrap();
    repo.update_canonical_email(id, &new_email).await.unwrap();

    // Login lookup key (password subject) is now the new email; old subject gone.
    assert!(repo.find_identity(AuthenticationProvider::Password, email.as_str()).await.unwrap().is_none());
    let idrec = repo
        .find_identity(AuthenticationProvider::Password, new_email.as_str())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(idrec.user_id, id);

    cleanup_user(&email).await;
    cleanup_user("renamed@bikenest.test").await;
}

#[db_test]
async fn session_store_create_resolve_expire_revoke(_tx: &mut bikenest_test_support::TestTx) {
    let db = Db::from_pool(pool().await);
    let store = SqlxSessionStore::new(db);
    let email = marker_email("session");
    cleanup_user(&email).await;
    let (user_id,) = sqlx::query_as("INSERT INTO users (email) VALUES ($1) RETURNING id")
        .bind(&email)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    let user_id = bikenest_domain::UserId(user_id);

    let raw = SessionId::new([7u8; 32]);
    let csrf = CsrfToken::new([9u8; 32]);
    let now = Utc::now();
    store.create(user_id, &raw, &csrf, now).await.unwrap();

    // Resolve succeeds and refreshes last_seen_at.
    let s = store.resolve(&raw, now).await.unwrap().unwrap();
    assert_eq!(s.user_id, user_id);
    assert_eq!(s.csrf_token.to_base64url(), csrf.to_base64url());

    // Absolute expiry: past the 90-day cap.
    assert!(store.resolve(&raw, now + Duration::days(91)).await.unwrap().is_none());
    // Idle expiry: 31 days without use.
    assert!(store.resolve(&raw, now + Duration::days(31)).await.unwrap().is_none());

    // Revoke.
    store.revoke(&raw).await.unwrap();
    assert!(store.resolve(&raw, now).await.unwrap().is_none());

    cleanup_user(&email).await;
}

#[db_test]
async fn token_store_single_use_is_atomic(_tx: &mut bikenest_test_support::TestTx) {
    let db = Db::from_pool(pool().await);
    let store = SqlxTokenStore::new(db);
    let email = marker_email("token");
    cleanup_user(&email).await;
    let (user_id,) = sqlx::query_as("INSERT INTO users (email) VALUES ($1) RETURNING id")
        .bind(&email)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    let user_id = bikenest_domain::UserId(user_id);
    let now = Utc::now();

    let raw = VerificationToken::new([42u8; 32]);
    store.issue_verification(user_id, &email, &raw, now).await.unwrap();

    // Two concurrent consumes: exactly one wins (atomic used_at guard).
    let (a, b) = tokio::join!(
        store.consume_verification(&raw, now),
        store.consume_verification(&raw, now),
    );
    let hits = [a, b].iter().filter(|r| matches!(r, Ok(Some(_)))).count();
    assert_eq!(hits, 1, "single-use guard must allow exactly one consume");

    // The consumed token is no longer usable.
    assert!(store.consume_verification(&raw, now).await.unwrap().is_none());

    // Reset token similarly single-use and short-lived.
    store.issue_reset(user_id, &raw, now).await.unwrap();
    assert!(store.consume_reset(&raw, now).await.unwrap().is_some());
    assert!(store.consume_reset(&raw, now).await.unwrap().is_none());

    cleanup_user(&email).await;
}

#[db_test]
async fn token_expiry_blocks_consumption_after_ttl(_tx: &mut bikenest_test_support::TestTx) {
    let db = Db::from_pool(pool().await);
    let store = SqlxTokenStore::new(db);
    let email = marker_email("expiry");
    cleanup_user(&email).await;
    let (user_id,) = sqlx::query_as("INSERT INTO users (email) VALUES ($1) RETURNING id")
        .bind(&email)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    let user_id = bikenest_domain::UserId(user_id);
    let now = Utc::now();
    let raw = VerificationToken::new([5u8; 32]);

    // Verification token: issued at `now`, TTL 24h — a consume at +25h is expired.
    store.issue_verification(user_id, &email, &raw, now).await.unwrap();
    assert!(store.consume_verification(&raw, now + Duration::hours(25)).await.unwrap().is_none());

    // Reset token: TTL 1h — a consume at +2h is expired.
    store.issue_reset(user_id, &raw, now).await.unwrap();
    assert!(store.consume_reset(&raw, now + Duration::hours(2)).await.unwrap().is_none());

    cleanup_user(&email).await;
}

#[db_test]
async fn audit_insert_round_trip(_tx: &mut bikenest_test_support::TestTx) {
    let db = Db::from_pool(pool().await);
    let audit = SqlxAuditLog::new(db);
    let email = marker_email("audit");
    cleanup_user(&email).await;
    let (user_id,) = sqlx::query_as("INSERT INTO users (email) VALUES ($1) RETURNING id")
        .bind(&email)
        .fetch_one(&pool().await)
        .await
        .unwrap();

    audit
        .record(AuditEvent::success(
            Some(bikenest_domain::UserId(user_id)),
            "auth.login",
            "user",
            user_id.to_string(),
        ))
        .await
        .unwrap();

    let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM audit_events WHERE actor_user_id = $1 AND action = 'auth.login'")
        .bind(user_id)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    assert_eq!(count, 1);

    cleanup_user(&email).await;
}
