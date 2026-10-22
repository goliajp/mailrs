//! Executable contract for [`QueueStore`] implementations.
//!
//! Call [`check_queue_store`] from a test in the crate that implements the
//! trait. It panics with the name of the first broken rule. Every rule is
//! one the bundled [`DeliveryWorker`](crate::DeliveryWorker) or the admin
//! surface depends on.
//!
//! ```no_run
//! # async fn run() {
//! use mailrs_outbound_queue::InMemoryQueueStore;
//! use mailrs_outbound_queue::store::contract::check_queue_store;
//!
//! check_queue_store(|| async { InMemoryQueueStore::new() }).await;
//! # }
//! ```

use std::future::Future;
use std::sync::Arc;

use super::{QueueStore, STALE_INFLIGHT_SECS};
use crate::queue::QueueStatus;

/// Run every contract rule against stores produced by `fresh`. Each rule
/// gets its own store, so `fresh` must return an empty one every time.
pub async fn check_queue_store<S, F, Fut>(fresh: F)
where
    S: QueueStore + 'static,
    F: Fn() -> Fut,
    Fut: Future<Output = S>,
{
    claim_moves_due_rows_to_inflight(&fresh().await).await;
    claim_skips_future_rows_and_respects_limit(&fresh().await).await;
    concurrent_claims_are_disjoint(Arc::new(fresh().await)).await;
    lifecycle_marks(&fresh().await).await;
    stale_inflight_is_recovered_fresh_is_not(&fresh().await).await;
    admin_operations(&fresh().await).await;
    suppression_list(&fresh().await).await;
}

async fn put(s: &impl QueueStore, rcpt: &str, now: i64) -> i64 {
    let message_id = format!("<{rcpt}>");
    s.enqueue(
        "s@example.com",
        rcpt,
        "dest.example",
        b"raw",
        Some(&message_id),
        now,
        false,
    )
    .await
    .expect("enqueue")
}

async fn status(s: &impl QueueStore, id: i64) -> QueueStatus {
    s.get_message(id)
        .await
        .expect("get_message")
        .expect("message exists")
        .status
}

async fn claim_moves_due_rows_to_inflight(s: &impl QueueStore) {
    let id = put(s, "r@dest.example", 0).await;
    let claimed = s.claim_for_delivery(5, 10).await.expect("claim");
    assert_eq!(claimed.len(), 1, "claim: a due pending row is claimed");
    assert_eq!(claimed[0].id, id);
    assert_eq!(
        claimed[0].status,
        QueueStatus::InFlight,
        "claim: returned as inflight"
    );
    assert_eq!(
        claimed[0].message_data, b"raw",
        "claim: carries the message"
    );
    assert_eq!(
        status(s, id).await,
        QueueStatus::InFlight,
        "claim: persisted as inflight"
    );
    assert!(
        s.claim_for_delivery(5, 10).await.expect("claim").is_empty(),
        "claim: an inflight row is not claimed again"
    );
}

async fn claim_skips_future_rows_and_respects_limit(s: &impl QueueStore) {
    let late = s
        .enqueue_scheduled(
            "s@example.com",
            "a@dest.example",
            "dest.example",
            b"",
            None,
            0,
            100,
        )
        .await
        .expect("enqueue_scheduled");
    let second = put(s, "b@dest.example", 20).await;
    let first = put(s, "c@dest.example", 10).await;
    let claimed = s.claim_for_delivery(50, 1).await.expect("claim");
    let ids: Vec<i64> = claimed.iter().map(|m| m.id).collect();
    assert_eq!(
        ids,
        vec![first],
        "claim: earliest next_retry first, at most `limit`"
    );
    let claimed = s.claim_for_delivery(50, 10).await.expect("claim");
    let ids: Vec<i64> = claimed.iter().map(|m| m.id).collect();
    assert_eq!(ids, vec![second], "claim: rows scheduled after `now` wait");
    let claimed = s.claim_for_delivery(100, 10).await.expect("claim");
    assert_eq!(
        claimed.len(),
        1,
        "claim: a scheduled row is due at its time"
    );
    assert_eq!(claimed[0].id, late);
}

async fn concurrent_claims_are_disjoint<S: QueueStore + 'static>(s: Arc<S>) {
    let mut expected = Vec::new();
    for i in 0..20 {
        expected.push(put(s.as_ref(), &format!("r{i}@dest.example"), 0).await);
    }
    let (a, b) = (s.clone(), s.clone());
    let ha = tokio::spawn(async move { a.claim_for_delivery(0, 100).await.expect("claim a") });
    let hb = tokio::spawn(async move { b.claim_for_delivery(0, 100).await.expect("claim b") });
    let (a, b) = (ha.await.expect("join a"), hb.await.expect("join b"));
    let mut all: Vec<i64> = a.iter().chain(b.iter()).map(|m| m.id).collect();
    all.sort_unstable();
    assert_eq!(
        all, expected,
        "claim: concurrent claims are disjoint and cover every row"
    );
}

async fn lifecycle_marks(s: &impl QueueStore) {
    let ok = put(s, "ok@dest.example", 0).await;
    let retry = put(s, "retry@dest.example", 0).await;
    let dead = put(s, "dead@dest.example", 0).await;
    s.claim_for_delivery(0, 10).await.expect("claim");

    s.mark_delivered(ok, 7).await.expect("mark_delivered");
    let m = s.get_message(ok).await.expect("get").expect("exists");
    assert_eq!(m.status, QueueStatus::Delivered, "mark_delivered: status");
    assert_eq!(m.updated_at, 7, "mark_delivered: updated_at");

    s.mark_failed(retry, "451 later", 600, 8)
        .await
        .expect("mark_failed");
    let m = s.get_message(retry).await.expect("get").expect("exists");
    assert_eq!(
        m.status,
        QueueStatus::Pending,
        "mark_failed: back to pending"
    );
    assert_eq!(m.attempts, 1, "mark_failed: attempts + 1");
    assert_eq!(
        m.last_error.as_deref(),
        Some("451 later"),
        "mark_failed: last_error"
    );
    assert_eq!(m.next_retry, 600, "mark_failed: next_retry");

    s.mark_bounced(dead, "550 no such user", 9)
        .await
        .expect("mark_bounced");
    let m = s.get_message(dead).await.expect("get").expect("exists");
    assert_eq!(m.status, QueueStatus::Bounced, "mark_bounced: status");
    assert_eq!(
        m.last_error.as_deref(),
        Some("550 no such user"),
        "mark_bounced: last_error"
    );

    assert!(
        s.get_message(i64::MAX).await.expect("get").is_none(),
        "get_message: unknown id"
    );

    let mut stats = s.queue_stats().await.expect("queue_stats");
    stats.sort();
    let want = [("bounced", 1), ("delivered", 1), ("pending", 1)];
    let want: Vec<(String, i64)> = want.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    assert_eq!(stats, want, "queue_stats: one count per status present");
}

async fn stale_inflight_is_recovered_fresh_is_not(s: &impl QueueStore) {
    let stale = put(s, "stale@dest.example", 0).await;
    s.claim_for_delivery(0, 10).await.expect("claim");
    let fresh = put(s, "fresh@dest.example", 500).await;
    s.claim_for_delivery(500, 10).await.expect("claim");

    let now = STALE_INFLIGHT_SECS + 100;
    let n = s.recover_stale_inflight(now).await.expect("recover");
    assert_eq!(n, 1, "recover: only rows idle past the threshold");
    assert_eq!(
        status(s, stale).await,
        QueueStatus::Pending,
        "recover: stale row is pending"
    );
    assert_eq!(
        status(s, fresh).await,
        QueueStatus::InFlight,
        "recover: live delivery untouched"
    );
    let again = s.claim_for_delivery(now, 10).await.expect("claim");
    assert_eq!(
        again.len(),
        1,
        "recover: the recovered row can be claimed again"
    );
    assert_eq!(again[0].id, stale);
}

async fn admin_operations(s: &impl QueueStore) {
    let a = put(s, "a@dest.example", 1).await;
    let b = put(s, "b@dest.example", 2).await;
    let c = put(s, "c@dest.example", 3).await;
    let recent: Vec<i64> = s
        .list_recent(2)
        .await
        .expect("list_recent")
        .iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(
        recent,
        vec![c, b],
        "list_recent: newest first, at most `limit`"
    );

    assert!(
        s.reschedule_pending(a, 900, 4).await.expect("reschedule"),
        "reschedule: pending row"
    );
    let m = s.get_message(a).await.expect("get").expect("exists");
    assert_eq!(m.next_retry, 900, "reschedule: next_retry moved");
    assert!(
        s.cancel_pending(a).await.expect("cancel"),
        "cancel_pending: pending row removed"
    );
    assert!(
        s.get_message(a).await.expect("get").is_none(),
        "cancel_pending: row is gone"
    );

    assert!(
        !s.cancel_pending_by_message_id("<b@dest.example>", "other@example.com")
            .await
            .expect("cancel"),
        "cancel_by_message_id: another sender cannot cancel"
    );
    assert!(
        s.cancel_pending_by_message_id("<b@dest.example>", "s@example.com")
            .await
            .expect("cancel"),
        "cancel_by_message_id: the sender can"
    );

    let claimed = s.claim_for_delivery(10, 10).await.expect("claim");
    let ids: Vec<i64> = claimed.iter().map(|m| m.id).collect();
    assert_eq!(
        ids,
        vec![c],
        "cancel_by_message_id: removed only that message"
    );
    assert!(
        !s.cancel_pending(claimed[0].id).await.expect("cancel"),
        "cancel_pending: not inflight"
    );
    assert!(
        !s.reschedule_pending(claimed[0].id, 5, 5)
            .await
            .expect("reschedule"),
        "reschedule: not inflight"
    );
    assert!(
        !s.retry_message(claimed[0].id, 5).await.expect("retry"),
        "retry_message: not inflight"
    );

    s.mark_bounced(claimed[0].id, "550", 20)
        .await
        .expect("mark_bounced");
    assert!(
        s.retry_message(claimed[0].id, 30).await.expect("retry"),
        "retry_message: bounced row"
    );
    let m = s
        .get_message(claimed[0].id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(
        m.status,
        QueueStatus::Pending,
        "retry_message: back to pending"
    );
    assert_eq!(m.next_retry, 30, "retry_message: due now");
}

async fn suppression_list(s: &impl QueueStore) {
    assert!(
        !s.is_suppressed("u@x.example").await.expect("is_suppressed"),
        "suppression: empty"
    );
    s.add_suppression("u@x.example", "550 gone", Some(550))
        .await
        .expect("add");
    s.add_suppression("v@x.example", "550 gone", None)
        .await
        .expect("add");
    s.add_suppression("u@x.example", "551 moved", Some(551))
        .await
        .expect("add again");
    assert!(
        s.is_suppressed("u@x.example").await.expect("is_suppressed"),
        "suppression: added"
    );

    let list = s.list_suppressions(10).await.expect("list");
    assert_eq!(
        list.len(),
        2,
        "suppression: re-adding replaces, no duplicate"
    );
    let u = list
        .iter()
        .find(|e| e.0 == "u@x.example")
        .expect("u listed");
    assert_eq!(
        (u.1.as_str(), u.2),
        ("551 moved", Some(551)),
        "suppression: reason replaced"
    );
    assert_eq!(
        s.list_suppressions(1).await.expect("list").len(),
        1,
        "suppression: limit"
    );

    assert!(
        s.remove_suppression("u@x.example").await.expect("remove"),
        "suppression: removed"
    );
    assert!(
        !s.remove_suppression("u@x.example").await.expect("remove"),
        "suppression: removed once"
    );
    assert!(
        !s.is_suppressed("u@x.example").await.expect("is_suppressed"),
        "suppression: gone"
    );
}
