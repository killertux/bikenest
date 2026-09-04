//! Integration tests for the PostgreSQL background job repository
//! (plans/m9-background-jobs.md).
//!
//! The repo operates on the shared pool, so each test seeds rows with a unique
//! `kind` prefix and cleans them up at the end (rows are not rolled back — they
//! are on the pool, not the test transaction). Where a test needs a "claimed"
//! row it simulates it with a direct `UPDATE` so it does not race other tests'
//! `claim` calls; the one real `claim` test asserts only the `SKIP LOCKED`
//! disjointness property, which holds regardless of concurrent claims.

use bikenest_infrastructure::{Db, JobConfig, JobRegistry, SqlxJobRepository, Worker};
use bikenest_test_support::{db_test, pool};
use chrono::{Duration, Utc};
use serde_json::json;
use tokio_util::sync::CancellationToken;

async fn db() -> Db {
    Db::from_pool(pool().await)
}

async fn repo() -> SqlxJobRepository {
    SqlxJobRepository::new(db().await)
}

/// Delete every background_job row whose kind starts with `jobtest.` (cleanup for
/// whatever this test created; other tests use their own distinct kinds).
async fn clear_kind(kind_prefix: &str) {
    sqlx::query("DELETE FROM background_job WHERE kind LIKE $1")
        .bind(format!("{kind_prefix}%"))
        .execute(&pool().await)
        .await
        .unwrap();
}

#[db_test]
async fn enqueue_is_idempotent_on_key(_tx: &mut bikenest_test_support::TestTx) {
    let r = repo().await;
    let now = Utc::now();
    // First insert with a stable key → Ok(Some(id)).
    let first = r
        .enqueue(
            "jobtest.idem",
            &json!({"n": 1}),
            now,
            Some(3),
            Some("recurring:jobtest.idem"),
        )
        .await
        .unwrap();
    assert!(first.is_some());
    // Second insert with the same key is a no-op → Ok(None).
    let second = r
        .enqueue(
            "jobtest.idem",
            &json!({"n": 2}),
            now,
            Some(3),
            Some("recurring:jobtest.idem"),
        )
        .await
        .unwrap();
    assert!(second.is_none(), "idempotency_key must dedup enqueue");
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM background_job WHERE kind = 'jobtest.idem'")
            .fetch_one(&pool().await)
            .await
            .unwrap();
    assert_eq!(n, 1);
    clear_kind("jobtest.idem").await;
}

#[db_test]
async fn finish_success_completes_oneshot(_tx: &mut bikenest_test_support::TestTx) {
    let r = repo().await;
    let now = Utc::now();
    let id = r
        .enqueue("jobtest.oneshot", &json!({}), now, Some(5), None)
        .await
        .unwrap()
        .unwrap();
    // Simulate a claim (state running, attempt 1).
    let claimed = sqlx::query(
        "UPDATE background_job SET state='running', claimed_by='w', lease_expires_at=now()+interval '60 seconds', attempts=1 WHERE id=$1",
    )
    .bind(id)
    .execute(&pool().await)
    .await
    .unwrap();
    assert_eq!(claimed.rows_affected(), 1);

    r.finish_success(id, "w", None, now).await.unwrap();

    let (state, finished): (String, Option<chrono::DateTime<Utc>>) =
        sqlx::query_as("SELECT state, finished_at FROM background_job WHERE id=$1")
            .bind(id)
            .fetch_one(&pool().await)
            .await
            .unwrap();
    assert_eq!(state, "succeeded");
    assert!(finished.is_some());
    clear_kind("jobtest.oneshot").await;
}

#[db_test]
async fn finish_success_reschedules_recurring(_tx: &mut bikenest_test_support::TestTx) {
    let r = repo().await;
    let now = Utc::now();
    let id = r
        .enqueue("jobtest.recurr", &json!({}), now, Some(5), None)
        .await
        .unwrap()
        .unwrap();
    // Claim + mark the row as recurring.
    sqlx::query(
        "UPDATE background_job SET state='running', claimed_by='w', schedule='{\"every_seconds\": 60}'::jsonb, attempts=1 WHERE id=$1",
    )
    .bind(id)
    .execute(&pool().await)
    .await
    .unwrap();

    let next = now + Duration::seconds(60);
    r.finish_success(id, "w", Some(next), now).await.unwrap();

    let (state, attempts, run_at): (String, i32, chrono::DateTime<Utc>) =
        sqlx::query_as("SELECT state, attempts, run_at FROM background_job WHERE id=$1")
            .bind(id)
            .fetch_one(&pool().await)
            .await
            .unwrap();
    assert_eq!(state, "pending");
    assert_eq!(attempts, 0, "recurring success resets the attempt budget");
    assert!(run_at > now, "next run is in the future");
    clear_kind("jobtest.recurr").await;
}

#[db_test]
async fn retry_then_dead_letter(_tx: &mut bikenest_test_support::TestTx) {
    let r = repo().await;
    let now = Utc::now();
    let id = r
        .enqueue("jobtest.retry", &json!({}), now, Some(2), None)
        .await
        .unwrap()
        .unwrap();
    sqlx::query(
        "UPDATE background_job SET state='running', claimed_by='w', attempts=1 WHERE id=$1",
    )
    .bind(id)
    .execute(&pool().await)
    .await
    .unwrap();

    // Attempt 1 < max(2) → retry (state pending, future run_at, last_error set).
    let run_at = now + Duration::seconds(60);
    r.retry(id, "w", "boom", run_at).await.unwrap();
    let (state, last_error): (String, Option<String>) =
        sqlx::query_as("SELECT state, last_error FROM background_job WHERE id=$1")
            .bind(id)
            .fetch_one(&pool().await)
            .await
            .unwrap();
    assert_eq!(state, "pending");
    assert_eq!(last_error.as_deref(), Some("boom"));

    // Attempt 2 == max(2) → dead-letter (state failed, finished_at set).
    sqlx::query(
        "UPDATE background_job SET state='running', claimed_by='w', attempts=2 WHERE id=$1",
    )
    .bind(id)
    .execute(&pool().await)
    .await
    .unwrap();
    r.fail(id, "w", "boom-again").await.unwrap();
    let (state, finished): (String, Option<chrono::DateTime<Utc>>) =
        sqlx::query_as("SELECT state, finished_at FROM background_job WHERE id=$1")
            .bind(id)
            .fetch_one(&pool().await)
            .await
            .unwrap();
    assert_eq!(state, "failed");
    assert!(finished.is_some());
    clear_kind("jobtest.retry").await;
}

#[db_test]
async fn gc_deletes_only_old_terminal_rows(_tx: &mut bikenest_test_support::TestTx) {
    let r = repo().await;
    let now = Utc::now();
    let cut_off = now - Duration::days(7);
    let id_old = r
        .enqueue("jobtest.gc_old", &json!({}), now, Some(5), None)
        .await
        .unwrap()
        .unwrap();
    let id_fresh = r
        .enqueue("jobtest.gc_fresh", &json!({}), now, Some(5), None)
        .await
        .unwrap()
        .unwrap();
    // terminal + old → should be deleted; terminal + fresh → kept; pending → kept.
    sqlx::query("UPDATE background_job SET state='succeeded', finished_at=$2 WHERE id=$1")
        .bind(id_old)
        .bind(now - Duration::days(10))
        .execute(&pool().await)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE background_job SET state='failed', finished_at=now(), last_error='x' WHERE id=$1",
    )
    .bind(id_fresh)
    .execute(&pool().await)
    .await
    .unwrap();
    let id_pending = r
        .enqueue("jobtest.gc_pending", &json!({}), now, Some(5), None)
        .await
        .unwrap()
        .unwrap();

    let deleted = r.gc(cut_off).await.unwrap();
    assert!(deleted >= 1, "returns the rows it removed");

    let gone: i64 = sqlx::query_scalar("SELECT count(*) FROM background_job WHERE id=$1")
        .bind(id_old)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    assert_eq!(gone, 0, "old terminal row is deleted");
    let fresh: i64 = sqlx::query_scalar("SELECT count(*) FROM background_job WHERE id=$1")
        .bind(id_fresh)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    assert_eq!(fresh, 1, "fresh terminal row is kept");
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM background_job WHERE id=$1")
        .bind(id_pending)
        .fetch_one(&pool().await)
        .await
        .unwrap();
    assert_eq!(pending, 1, "pending row is never deleted");
    clear_kind("jobtest.gc").await;
}

#[db_test]
async fn concurrent_claims_are_disjoint(_tx: &mut bikenest_test_support::TestTx) {
    let r = repo().await;
    let now = Utc::now();
    // Seed a handful of due, unique-kind rows.
    let mut ids = Vec::new();
    for i in 0..6 {
        let id = r
            .enqueue("jobtest.disjoint", &json!({"i": i}), now, Some(5), None)
            .await
            .unwrap()
            .unwrap();
        ids.push(id);
    }

    // Two workers claim concurrently, each with a large batch so the whole due
    // set is covered (avoids a concurrent test's due rows crowding ours out).
    let repo_a = r.clone();
    let repo_b = r.clone();
    let a = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let b = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let (a2, b2) = (a.clone(), b.clone());
    let h1 = tokio::spawn(async move {
        let got = repo_a
            .claim(1000, "worker-a", std::time::Duration::from_secs(60))
            .await
            .unwrap();
        *a2.lock().await = got;
    });
    let h2 = tokio::spawn(async move {
        let got = repo_b
            .claim(1000, "worker-b", std::time::Duration::from_secs(60))
            .await
            .unwrap();
        *b2.lock().await = got;
    });
    let _ = (h1.await.unwrap(), h2.await.unwrap());
    let claim_a = std::mem::take(&mut *a.lock().await);
    let claim_b = std::mem::take(&mut *b.lock().await);

    // SKIP LOCKED → the two claims never share an id.
    let ids_a: std::collections::HashSet<i64> = claim_a.iter().map(|j| j.id).collect();
    let ids_b: std::collections::HashSet<i64> = claim_b.iter().map(|j| j.id).collect();
    assert!(
        ids_a.is_disjoint(&ids_b),
        "two workers must never claim the same job ({ids_a:?} vs {ids_b:?})"
    );

    // Each of OUR rows was claimed exactly once (state running, attempts 1, held by a
    // worker). We only assert on our own rows: the claim also picks up OTHER
    // concurrently-running tests' due rows, which those tests may delete before we
    // inspect them (hence no assertion over the full union).
    for id in &ids {
        let (state, attempts, claimed_by): (String, i32, Option<String>) =
            sqlx::query_as("SELECT state, attempts, claimed_by FROM background_job WHERE id=$1")
                .bind(id)
                .fetch_one(&pool().await)
                .await
                .unwrap();
        assert_eq!(state, "running");
        assert_eq!(attempts, 1);
        assert!(claimed_by.is_some(), "claimed row must be held by a worker");
    }
    // Note: a row can end up claimed by neither `worker-a` nor `worker-b` if some
    // other concurrently-running test also calls `claim` (it is not scoped by
    // kind) and wins the race for it first — the per-row loop above already
    // confirms every seeded row was claimed by *someone*, which is the
    // meaningful invariant; we don't additionally require it be one of this
    // test's own two workers.
    clear_kind("jobtest.disjoint").await;
}

#[db_test]
async fn claim_reclaims_a_crashed_workers_running_job(_tx: &mut bikenest_test_support::TestTx) {
    let r = repo().await;
    let now = Utc::now();
    let id = r
        .enqueue("jobtest.reclaim", &json!({}), now, Some(5), None)
        .await
        .unwrap()
        .unwrap();
    // Simulate a worker that claimed the job and then crashed: state left
    // 'running' with a lease that already expired.
    sqlx::query(
        "UPDATE background_job SET state='running', claimed_by='dead-worker',
            lease_expires_at=now() - interval '1 second', attempts=1 WHERE id=$1",
    )
    .bind(id)
    .execute(&pool().await)
    .await
    .unwrap();

    // `claim` is not scoped by kind, so — since this row is now a candidate —
    // a concurrently-running test's own (larger) claim batch could in theory
    // win the race and reclaim it before this call does. Either way proves the
    // property under test (a running job past its lease is reclaimable), so
    // the assertions below check the row's resulting state rather than
    // requiring that *this* call was the one that claimed it.
    let claimed = r
        .claim(10, "worker-b", std::time::Duration::from_secs(60))
        .await
        .unwrap();
    let we_claimed_it = claimed.iter().any(|j| j.id == id);

    let (state, claimed_by, attempts, lease_expires_at): (
        String,
        Option<String>,
        i32,
        Option<chrono::DateTime<Utc>>,
    ) = sqlx::query_as(
        "SELECT state, claimed_by, attempts, lease_expires_at FROM background_job WHERE id=$1",
    )
    .bind(id)
    .fetch_one(&pool().await)
    .await
    .unwrap();
    assert_eq!(state, "running");
    assert_ne!(
        claimed_by.as_deref(),
        Some("dead-worker"),
        "the crashed worker's stale claim must have been superseded"
    );
    assert!(attempts >= 2, "reclaim increments attempts again");
    assert!(
        lease_expires_at.is_some_and(|t| t > Utc::now()),
        "the reclaiming worker holds a fresh, unexpired lease"
    );
    if we_claimed_it {
        assert_eq!(claimed_by.as_deref(), Some("worker-b"));
        assert_eq!(attempts, 2);
    }

    // A second claim right after must NOT pick it up again — it now holds a
    // fresh, unexpired lease (held by whichever worker won the reclaim).
    let claimed_again = r
        .claim(10, "worker-c", std::time::Duration::from_secs(60))
        .await
        .unwrap();
    assert!(
        !claimed_again.iter().any(|j| j.id == id),
        "a freshly (re)claimed job must not be claimed again"
    );

    clear_kind("jobtest.reclaim").await;
}

#[db_test]
async fn finish_success_is_a_noop_for_the_wrong_claimant(_tx: &mut bikenest_test_support::TestTx) {
    let r = repo().await;
    let now = Utc::now();
    let id = r
        .enqueue("jobtest.zombie", &json!({}), now, Some(5), None)
        .await
        .unwrap()
        .unwrap();
    // The row is currently (re)claimed by "worker-b" (as if worker-a's original
    // claim expired and was reassigned).
    sqlx::query(
        "UPDATE background_job SET state='running', claimed_by='worker-b',
            lease_expires_at=now()+interval '60 seconds', attempts=2 WHERE id=$1",
    )
    .bind(id)
    .execute(&pool().await)
    .await
    .unwrap();

    // The zombie worker-a wakes up and tries to finish its stale claim.
    r.finish_success(id, "worker-a", None, now).await.unwrap();

    let (state, claimed_by): (String, Option<String>) =
        sqlx::query_as("SELECT state, claimed_by FROM background_job WHERE id=$1")
            .bind(id)
            .fetch_one(&pool().await)
            .await
            .unwrap();
    assert_eq!(state, "running", "wrong claimant's write must not apply");
    assert_eq!(claimed_by.as_deref(), Some("worker-b"));

    clear_kind("jobtest.zombie").await;
}

/// Graceful shutdown (WP7): cancelling the token while the worker sits in its
/// idle poll must return from `run` well inside one poll interval — not after
/// it — and must leave nothing claimed.
///
/// `batch_size` 0 makes `claim` a `LIMIT 0` query, so the worker only ever
/// idle-polls and cannot disturb rows other tests own.
#[db_test]
async fn cancelling_the_token_stops_an_idle_worker(_tx: &mut bikenest_test_support::TestTx) {
    let config = JobConfig {
        enabled: true,
        // Far longer than the test may take: if cancellation did not interrupt
        // the sleep, the timeout below would fire instead.
        poll_interval: std::time::Duration::from_secs(60),
        batch_size: 0,
        ..JobConfig::default()
    };
    let worker = Worker::new(
        repo().await,
        std::sync::Arc::new(JobRegistry::new(Vec::new(), Vec::new())),
        config,
    );
    let worker_id = worker.id().to_string();

    let token = CancellationToken::new();
    let handle = tokio::spawn(worker.run(token.clone()));
    // Let the loop reach its idle sleep before signalling.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let started = std::time::Instant::now();
    token.cancel();
    tokio::time::timeout(std::time::Duration::from_secs(5), handle)
        .await
        .expect("run must return promptly after cancellation, not after the poll interval")
        .expect("worker task must not panic");
    assert!(
        started.elapsed() < config.poll_interval,
        "returned only after the full poll interval: {:?}",
        started.elapsed()
    );

    let running: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM background_job WHERE state = 'running' AND claimed_by = $1",
    )
    .bind(&worker_id)
    .fetch_one(&pool().await)
    .await
    .unwrap();
    assert_eq!(running, 0, "a stopped worker must leave no job running");
}
