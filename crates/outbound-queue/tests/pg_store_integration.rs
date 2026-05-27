//! `PgQueueStore` trait-level integration tests.
//!
//! These exercise every method on the `QueueStore` trait through the
//! `PgQueueStore` impl in `pg_store.rs`. Tests for the lower-level
//! `queue::*` SQL functions live in `worker_integration.rs`; the
//! goal here is to make sure the `pg_store.rs` wrapper layer faithfully
//! delegates and that the trait surface itself (`async_trait` boxing,
//! `&dyn QueueStore` dispatch, `StoreError` mapping) works end-to-end.

mod common;

use common::pg::start_pg;
use mailrs_outbound_queue::PgQueueStore;
use mailrs_outbound_queue::queue::QueueStatus;
use mailrs_outbound_queue::store::QueueStore;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enqueue_then_get_roundtrips_via_trait() {
    let (_c, pool) = start_pg().await;
    let store: Box<dyn QueueStore> = Box::new(PgQueueStore::new(pool));

    let id = store
        .enqueue("s@x", "r@y", "y", b"raw bytes", Some("<msg-1@x>"), 42, true)
        .await
        .expect("enqueue");

    let row = store.get_message(id).await.unwrap().unwrap();
    assert_eq!(row.id, id);
    assert_eq!(row.sender, "s@x");
    assert_eq!(row.recipient, "r@y");
    assert_eq!(row.domain, "y");
    assert_eq!(row.message_data, b"raw bytes");
    assert_eq!(row.message_id.as_deref(), Some("<msg-1@x>"));
    assert_eq!(row.status, QueueStatus::Pending);
    assert!(row.is_forwarded);
    assert_eq!(row.created_at, 42);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enqueue_scheduled_uses_future_next_retry() {
    let (_c, pool) = start_pg().await;
    let store = PgQueueStore::new(pool);

    let created = 100;
    let scheduled = 10_000;
    let id = store
        .enqueue_scheduled("s@x", "r@y", "y", b"raw", None, created, scheduled)
        .await
        .expect("enqueue_scheduled");

    let row = store.get_message(id).await.unwrap().unwrap();
    assert_eq!(row.created_at, created);
    assert_eq!(row.next_retry, scheduled);

    // dequeue at created_at sees nothing — next_retry is in the future
    let early = store.dequeue(created, 10).await.unwrap();
    assert!(early.is_empty(), "scheduled message is invisible before its time");

    // dequeue at scheduled_at returns it
    let on_time = store.dequeue(scheduled, 10).await.unwrap();
    assert_eq!(on_time.len(), 1);
    assert_eq!(on_time[0].id, id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lifecycle_inflight_delivered_via_trait() {
    let (_c, pool) = start_pg().await;
    let store = PgQueueStore::new(pool);

    let id = store
        .enqueue("s@x", "r@y", "y", b"raw", None, 0, false)
        .await
        .unwrap();
    store.mark_inflight(id, 10).await.unwrap();
    let row = store.get_message(id).await.unwrap().unwrap();
    assert_eq!(row.status, QueueStatus::InFlight);
    assert_eq!(row.updated_at, 10);

    store.mark_delivered(id, 20).await.unwrap();
    let row = store.get_message(id).await.unwrap().unwrap();
    assert_eq!(row.status, QueueStatus::Delivered);
    assert_eq!(row.updated_at, 20);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mark_bounced_is_terminal() {
    let (_c, pool) = start_pg().await;
    let store = PgQueueStore::new(pool);

    let id = store.enqueue("s@x", "r@y", "y", b"raw", None, 0, false).await.unwrap();
    store.mark_inflight(id, 5).await.unwrap();
    store.mark_bounced(id, "550 user unknown", 10).await.unwrap();

    let row = store.get_message(id).await.unwrap().unwrap();
    assert_eq!(row.status, QueueStatus::Bounced);
    assert_eq!(row.last_error.as_deref(), Some("550 user unknown"));
    assert_eq!(row.updated_at, 10);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queue_stats_groups_by_status() {
    let (_c, pool) = start_pg().await;
    let store = PgQueueStore::new(pool);

    let p1 = store.enqueue("s@x", "r1@y", "y", b"", None, 0, false).await.unwrap();
    let _p2 = store.enqueue("s@x", "r2@y", "y", b"", None, 0, false).await.unwrap();
    let p3 = store.enqueue("s@x", "r3@y", "y", b"", None, 0, false).await.unwrap();
    store.mark_inflight(p1, 1).await.unwrap();
    store.mark_inflight(p3, 1).await.unwrap();
    store.mark_delivered(p3, 2).await.unwrap();

    let stats: std::collections::HashMap<String, i64> = store
        .queue_stats()
        .await
        .unwrap()
        .into_iter()
        .collect();
    assert_eq!(stats.get("pending").copied().unwrap_or(0), 1);
    assert_eq!(stats.get("inflight").copied().unwrap_or(0), 1);
    assert_eq!(stats.get("delivered").copied().unwrap_or(0), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_recent_orders_newest_first() {
    let (_c, pool) = start_pg().await;
    let store = PgQueueStore::new(pool);

    let a = store.enqueue("s@x", "r@y", "y", b"a", None, 100, false).await.unwrap();
    let b = store.enqueue("s@x", "r@y", "y", b"b", None, 200, false).await.unwrap();
    let c = store.enqueue("s@x", "r@y", "y", b"c", None, 300, false).await.unwrap();

    let recent = store.list_recent(10).await.unwrap();
    let ids: Vec<i64> = recent.iter().map(|m| m.id).collect();
    assert_eq!(ids, vec![c, b, a], "list_recent is newest-first by created_at");

    let limited = store.list_recent(2).await.unwrap();
    assert_eq!(limited.len(), 2, "list_recent honours limit");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_pending_removes_only_pending() {
    let (_c, pool) = start_pg().await;
    let store = PgQueueStore::new(pool);

    let pending = store.enqueue("s@x", "r@y", "y", b"", None, 0, false).await.unwrap();
    let inflight = store.enqueue("s@x", "r@y", "y", b"", None, 0, false).await.unwrap();
    store.mark_inflight(inflight, 1).await.unwrap();

    assert!(store.cancel_pending(pending).await.unwrap(), "pending row cancelled");
    assert!(store.get_message(pending).await.unwrap().is_none(), "pending row removed");

    assert!(
        !store.cancel_pending(inflight).await.unwrap(),
        "cancel_pending refuses non-pending rows"
    );
    assert!(store.get_message(inflight).await.unwrap().is_some(), "inflight row kept");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_pending_by_message_id_scopes_to_sender() {
    let (_c, pool) = start_pg().await;
    let store = PgQueueStore::new(pool);

    let _mine = store
        .enqueue("me@x", "r@y", "y", b"", Some("<abc@x>"), 0, false)
        .await
        .unwrap();
    let _someone_else = store
        .enqueue("you@x", "r@y", "y", b"", Some("<abc@x>"), 0, false)
        .await
        .unwrap();

    // wrong sender, no removal
    assert!(
        !store
            .cancel_pending_by_message_id("<abc@x>", "stranger@x")
            .await
            .unwrap()
    );
    // right sender removes only my row
    assert!(
        store
            .cancel_pending_by_message_id("<abc@x>", "me@x")
            .await
            .unwrap()
    );

    let all = store.list_recent(10).await.unwrap();
    let senders: Vec<&str> = all.iter().map(|m| m.sender.as_str()).collect();
    assert_eq!(senders, vec!["you@x"], "only the matching-sender row was removed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retry_message_resets_bounced_to_pending() {
    let (_c, pool) = start_pg().await;
    let store = PgQueueStore::new(pool);

    let id = store.enqueue("s@x", "r@y", "y", b"", None, 0, false).await.unwrap();
    store.mark_inflight(id, 1).await.unwrap();
    store.mark_bounced(id, "550 nope", 2).await.unwrap();

    assert!(store.retry_message(id, 100).await.unwrap(), "bounced → pending");
    let row = store.get_message(id).await.unwrap().unwrap();
    assert_eq!(row.status, QueueStatus::Pending);
    assert_eq!(row.next_retry, 100);

    // delivered rows are NOT eligible for retry
    let d = store.enqueue("s@x", "r@y", "y", b"", None, 0, false).await.unwrap();
    store.mark_delivered(d, 5).await.unwrap();
    assert!(!store.retry_message(d, 200).await.unwrap(), "delivered rows refuse retry");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn suppression_crud_via_trait() {
    let (_c, pool) = start_pg().await;
    let store = PgQueueStore::new(pool);

    assert!(!store.is_suppressed("a@x").await);

    store
        .add_suppression("a@x", "550 mailbox unavailable", Some(550))
        .await
        .unwrap();
    assert!(store.is_suppressed("a@x").await);

    // upsert: re-add updates reason + smtp_code without erroring
    store
        .add_suppression("a@x", "550 user unknown", Some(550))
        .await
        .unwrap();

    store.add_suppression("b@x", "complaint", None).await.unwrap();
    let listed = store.list_suppressions(100).await.unwrap();
    let emails: Vec<&str> = listed.iter().map(|(e, _, _, _)| e.as_str()).collect();
    assert!(emails.contains(&"a@x"));
    assert!(emails.contains(&"b@x"));
    let a_row = listed.iter().find(|(e, _, _, _)| e == "a@x").unwrap();
    assert_eq!(a_row.1, "550 user unknown", "upsert overwrote reason");
    assert_eq!(a_row.2, Some(550));

    // remove + idempotent re-remove
    assert!(store.remove_suppression("a@x").await.unwrap());
    assert!(!store.is_suppressed("a@x").await);
    assert!(!store.remove_suppression("a@x").await.unwrap(), "second remove is a no-op");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dequeue_legacy_returns_only_pending_due() {
    let (_c, pool) = start_pg().await;
    let store = PgQueueStore::new(pool);

    let now_due = store.enqueue("s@x", "r@y", "y", b"", None, 100, false).await.unwrap();
    let _future = store
        .enqueue_scheduled("s@x", "r@y", "y", b"", None, 100, 5_000)
        .await
        .unwrap();
    let inflight = store.enqueue("s@x", "r@y", "y", b"", None, 100, false).await.unwrap();
    store.mark_inflight(inflight, 101).await.unwrap();

    // legacy dequeue at t=200: only the immediately-due pending row
    let due = store.dequeue(200, 10).await.unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].id, now_due);
    assert_eq!(due[0].status, QueueStatus::Pending, "legacy dequeue does NOT transition status");
}
