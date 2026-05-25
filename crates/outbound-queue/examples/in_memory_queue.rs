//! End-to-end example using the trait surface against the in-memory store.
//! No database required.
//!
//! Run with: `cargo run -p mailrs-outbound-queue --example in_memory_queue`
//!
//! Walks the lifecycle: enqueue → dequeue → mark inflight → mark delivered.
//! Also demonstrates the suppression list and the retry primitives without
//! actually opening any sockets.

use std::sync::Arc;

use mailrs_outbound_queue::{
    InMemoryQueueStore, QueueStatus, QueueStore, retry_delay_secs, should_bounce,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store: Arc<dyn QueueStore> = Arc::new(InMemoryQueueStore::new());

    let now = chrono::Utc::now().timestamp();

    // 1. enqueue two messages
    let id1 = store
        .enqueue(
            "alice@x.org",
            "bob@y.com",
            "y.com",
            b"hi bob",
            None,
            now,
            false,
        )
        .await?;
    let id2 = store
        .enqueue(
            "alice@x.org",
            "carol@y.com",
            "y.com",
            b"hi carol",
            None,
            now,
            false,
        )
        .await?;
    println!("enqueued #{id1}, #{id2}");

    // 2. drain ready work
    let pending = store.dequeue(now, 50).await?;
    println!("dequeued {} message(s) ready for delivery", pending.len());

    // 3. mark first one delivered
    store.mark_inflight(id1, now).await?;
    store.mark_delivered(id1, now + 1).await?;

    // 4. mark second one failed once → goes back to pending with backoff
    let attempts_after = 1;
    let delay = retry_delay_secs(attempts_after);
    store
        .mark_failed(
            id2,
            "transient error: server too busy",
            now + delay as i64,
            now,
        )
        .await?;
    println!("retry delay after {attempts_after} attempt(s): {delay}s");

    // 5. add a suppressed recipient and check
    store
        .add_suppression("blocked@y.com", "hard bounce", Some(550))
        .await?;
    assert!(store.is_suppressed("blocked@y.com").await);
    assert!(!store.is_suppressed("alice@x.org").await);

    // 6. inspect: should_bounce returns true once attempts exceed max
    assert!(!should_bounce(1, 8));
    assert!(should_bounce(9, 8));

    // 7. queue stats
    println!("\nqueue stats:");
    for (status, count) in store.queue_stats().await? {
        println!("  {status:>10}  {count}");
    }

    // 8. verify lifecycle
    let m1 = store.get_message(id1).await?.unwrap();
    let m2 = store.get_message(id2).await?.unwrap();
    assert_eq!(m1.status, QueueStatus::Delivered);
    assert_eq!(m2.status, QueueStatus::Pending);
    assert_eq!(m2.attempts, 1);

    println!("\nall trait-surface ops worked against the in-memory store.");
    Ok(())
}
