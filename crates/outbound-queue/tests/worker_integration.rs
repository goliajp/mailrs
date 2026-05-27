//! Worker-path integration tests: the exact PG hot paths the bundled
//! `DeliveryWorker` exercises (atomic claim + lifecycle marks + crash
//! recovery + multi-worker race correctness).
//!
//! Each test starts a fresh container via `common::pg::start_pg` and
//! interacts with `queue::*` functions directly — these are the same
//! entry points `DeliveryWorker::poll_and_deliver` calls. Suppression
//! filtering (also worker-path) lives inline in `worker/delivery.rs`
//! so it can see private helpers; the test below it deliberately
//! does not duplicate it here.

mod common;

use common::pg::start_pg;
use mailrs_outbound_queue::queue::{self, QueueStatus};

/// Single-worker happy path: enqueue → atomic claim transitions to
/// `inflight` → terminal `mark_delivered` flips to `delivered`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn claim_for_delivery_then_mark_delivered() {
    let (_c, pool) = start_pg().await;

    let id = queue::enqueue_ex(&pool, "s@example.com", "r@dest.com", "dest.com", b"raw", None, 0, false)
        .await
        .expect("enqueue");

    let claimed = queue::claim_for_delivery(&pool, 0, 10).await.expect("claim");
    assert_eq!(claimed.len(), 1, "exactly one row claimed");
    assert_eq!(claimed[0].id, id);
    assert_eq!(claimed[0].status, QueueStatus::InFlight, "claim atomically transitions to inflight");

    let after_claim = queue::get_message(&pool, id).await.unwrap().unwrap();
    assert_eq!(after_claim.status, QueueStatus::InFlight, "row persisted as inflight");

    queue::mark_delivered(&pool, id, 100).await.expect("mark_delivered");
    let after_deliver = queue::get_message(&pool, id).await.unwrap().unwrap();
    assert_eq!(after_deliver.status, QueueStatus::Delivered);
    assert_eq!(after_deliver.updated_at, 100);
}

/// A second `claim_for_delivery` call after the first claim sees no
/// rows — the SKIP LOCKED / inflight transition removed it from the
/// pending pool.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_claim_after_inflight_returns_empty() {
    let (_c, pool) = start_pg().await;

    queue::enqueue_ex(&pool, "s@example.com", "r@dest.com", "dest.com", b"raw", None, 0, false)
        .await
        .expect("enqueue");

    let first = queue::claim_for_delivery(&pool, 0, 10).await.expect("first claim");
    assert_eq!(first.len(), 1);

    let second = queue::claim_for_delivery(&pool, 0, 10).await.expect("second claim");
    assert!(second.is_empty(), "inflight rows are not re-claimed");
}

/// Crash recovery: a worker claims a row, "crashes" (no terminal
/// mark), and `recover_stale_inflight` after the 10-minute threshold
/// flips it back to `pending` so the next worker poll re-claims it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crash_recovery_via_stale_inflight() {
    let (_c, pool) = start_pg().await;

    // Stale-threshold math: recover_stale_inflight uses `now - 600` —
    // anything with updated_at < that line is recovered. Set t0=0 so
    // the claim writes updated_at=0, then run recovery at t=700.
    let id = queue::enqueue_ex(&pool, "s@example.com", "r@dest.com", "dest.com", b"raw", None, 0, false)
        .await
        .expect("enqueue");

    let claimed = queue::claim_for_delivery(&pool, 0, 10).await.expect("claim");
    assert_eq!(claimed.len(), 1);
    // simulate "worker crashed": no mark_delivered / mark_failed call

    // 10 minutes + 100s past the claim time
    let recovered = queue::recover_stale_inflight(&pool, 700).await.expect("recover");
    assert_eq!(recovered, 1, "stale inflight is recovered");

    let after = queue::get_message(&pool, id).await.unwrap().unwrap();
    assert_eq!(after.status, QueueStatus::Pending, "row is back to pending");
    assert_eq!(after.updated_at, 700);

    // re-claim with now=700 picks it up
    let reclaimed = queue::claim_for_delivery(&pool, 700, 10).await.expect("re-claim");
    assert_eq!(reclaimed.len(), 1);
    assert_eq!(reclaimed[0].id, id);
}

/// `recover_stale_inflight` MUST NOT touch fresh inflight rows
/// (updated_at >= now - 600). Otherwise a slow but live delivery
/// would get double-claimed by another worker.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recover_stale_inflight_leaves_fresh_rows_alone() {
    let (_c, pool) = start_pg().await;

    queue::enqueue_ex(&pool, "s@example.com", "r@dest.com", "dest.com", b"raw", None, 500, false)
        .await
        .expect("enqueue");
    let claimed = queue::claim_for_delivery(&pool, 500, 10).await.expect("claim");
    assert_eq!(claimed.len(), 1);

    // only 100s after claim — well under the 600s threshold
    let recovered = queue::recover_stale_inflight(&pool, 600).await.expect("recover");
    assert_eq!(recovered, 0, "fresh inflight is not touched");

    let still_inflight = queue::get_message(&pool, claimed[0].id).await.unwrap().unwrap();
    assert_eq!(still_inflight.status, QueueStatus::InFlight);
}

/// Two workers claim concurrently against 10 pending rows. The
/// `SKIP LOCKED` clause guarantees disjoint claims — union must
/// cover every id, intersection must be empty. The exact split
/// (5+5, 7+3, 10+0, …) is non-deterministic and that's OK.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_workers_no_double_claim() {
    let (_c, pool) = start_pg().await;

    let mut expected: Vec<i64> = Vec::with_capacity(10);
    for i in 0..10 {
        let id = queue::enqueue_ex(
            &pool,
            "s@example.com",
            &format!("r{i}@dest.com"),
            "dest.com",
            format!("raw {i}").as_bytes(),
            None,
            0,
            false,
        )
        .await
        .expect("enqueue");
        expected.push(id);
    }
    expected.sort();

    let pool_a = pool.clone();
    let pool_b = pool.clone();
    let (a, b) = tokio::join!(
        async move { queue::claim_for_delivery(&pool_a, 0, 100).await.expect("claim a") },
        async move { queue::claim_for_delivery(&pool_b, 0, 100).await.expect("claim b") },
    );

    let mut ids_a: Vec<i64> = a.iter().map(|m| m.id).collect();
    let mut ids_b: Vec<i64> = b.iter().map(|m| m.id).collect();
    ids_a.sort();
    ids_b.sort();

    // disjoint
    for id in &ids_a {
        assert!(!ids_b.contains(id), "id {id} double-claimed");
    }

    // union covers everything
    let mut union: Vec<i64> = ids_a.iter().copied().chain(ids_b.iter().copied()).collect();
    union.sort();
    assert_eq!(union, expected, "every pending id ended up in exactly one worker");

    // all 10 are now inflight
    let still_pending = queue::dequeue(&pool, 0, 100).await.expect("dequeue");
    assert!(
        still_pending.is_empty(),
        "no pending rows remain after both workers claimed"
    );
}

/// Failed attempt increments `attempts` and resets status to
/// `pending` with the supplied `next_retry`. The retry math itself
/// is unit-tested in `retry.rs`; this verifies the SQL transition.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mark_failed_increments_attempts_and_reschedules() {
    let (_c, pool) = start_pg().await;

    let id = queue::enqueue_ex(&pool, "s@example.com", "r@dest.com", "dest.com", b"raw", None, 0, false)
        .await
        .expect("enqueue");
    queue::claim_for_delivery(&pool, 0, 10).await.expect("claim");

    queue::mark_failed(&pool, id, "451 try again later", 600, 10)
        .await
        .expect("mark_failed");

    let row = queue::get_message(&pool, id).await.unwrap().unwrap();
    assert_eq!(row.status, QueueStatus::Pending, "failed → pending for retry");
    assert_eq!(row.attempts, 1);
    assert_eq!(row.last_error.as_deref(), Some("451 try again later"));
    assert_eq!(row.next_retry, 600);
}
