//! Database-backed integration tests against real PostgreSQL (§49).
//!
//! Each test runs on the shared multi-threaded runtime via `#[db_test]`,
//! receives an open transaction, and rolls back on completion (§50).

use bikenest_test_support::{UserBuilder, db_test};

#[db_test]
async fn user_builder_inserts_and_reads_back_within_test_transaction(
    tx: &mut bikenest_test_support::TestTx,
) {
    let user = UserBuilder::new()
        .with_email("Ada@Example.com")
        .with_name("Ada")
        .create(tx.executor())
        .await
        .expect("insert user");

    assert_eq!(user.email.as_str(), "ada@example.com");
    assert_eq!(user.display_name.as_deref(), Some("Ada"));

    let stored: (String, Option<String>) =
        sqlx::query_as("SELECT email, display_name FROM users WHERE id = $1")
            .bind(user.id.0)
            .fetch_one(tx.executor())
            .await
            .expect("read back");

    assert_eq!(stored.0, "ada@example.com");
    assert_eq!(stored.1.as_deref(), Some("Ada"));
    // No cleanup needed: the transaction rolls back after the test (§50).
}

#[db_test]
async fn duplicate_email_across_case_is_rejected_by_unique_index(
    tx: &mut bikenest_test_support::TestTx,
) {
    UserBuilder::new()
        .with_email("duplicate@example.com")
        .create(tx.executor())
        .await
        .expect("first insert");

    let second = UserBuilder::new()
        .with_email("DUPLICATE@example.com")
        .create(tx.executor())
        .await;

    let err = second.expect_err("unique index must reject case-insensitive duplicate");
    assert!(
        err.as_database_error()
            .map(|e| e.is_unique_violation())
            .unwrap_or(false),
        "expected unique violation, got: {err}"
    );
}

#[db_test]
async fn savepoint_allows_nested_transaction_with_inner_rollback(
    tx: &mut bikenest_test_support::TestTx,
) {
    // Outer insert survives.
    UserBuilder::new()
        .with_email("outer@example.com")
        .create(tx.executor())
        .await
        .expect("outer insert");

    {
        // Inner savepoint: insert then roll back to the savepoint (§51).
        let mut sp = tx.savepoint().await;
        UserBuilder::new()
            .with_email("inner@example.com")
            .create(sp.executor())
            .await
            .expect("inner insert");
        sp.rollback().await; // undo inner insert (§51)
    }

    let (inner_gone,): (i64,) = sqlx::query_as("SELECT count(*) FROM users WHERE email = $1")
        .bind("inner@example.com")
        .fetch_one(tx.executor())
        .await
        .unwrap();
    let (outer_stays,): (i64,) = sqlx::query_as("SELECT count(*) FROM users WHERE email = $1")
        .bind("outer@example.com")
        .fetch_one(tx.executor())
        .await
        .unwrap();

    assert_eq!(inner_gone, 0, "savepoint rollback must undo inner insert");
    assert_eq!(outer_stays, 1, "outer transaction must keep its insert");
}

#[db_test]
async fn savepoint_inner_commit_releases_savepoint(tx: &mut bikenest_test_support::TestTx) {
    {
        let mut sp = tx.savepoint().await;
        UserBuilder::new()
            .with_email("committed@example.com")
            .create(sp.executor())
            .await
            .expect("inner insert");
        sp.commit().await; // release savepoint
    }

    let (kept,): (i64,) = sqlx::query_as("SELECT count(*) FROM users WHERE email = $1")
        .bind("committed@example.com")
        .fetch_one(tx.executor())
        .await
        .unwrap();
    assert_eq!(kept, 1, "inner commit must keep the row within the test tx");
}
