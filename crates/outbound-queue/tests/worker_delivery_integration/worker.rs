//! The worker loop: a notification wakes it, and it carries a claimed row
//! through delivery to a terminal mark.

use std::sync::Arc;
use std::time::Duration;

use mailrs_outbound_queue::queue::QueueStatus;
use mailrs_outbound_queue::worker::{DeliveryWorker, WorkerConfig};
use mailrs_outbound_queue::{InMemoryNotifier, InMemoryQueueStore, Notifier, QueueStore};

use super::resolver;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_notification_wakes_the_worker_and_the_row_is_attempted() {
    let store = Arc::new(InMemoryQueueStore::new());
    let notifier = Arc::new(InMemoryNotifier::new());
    // the worker always connects on port 25; nothing answers there for
    // 127.0.0.1 in a test run, so the attempt ends in `mark_failed`
    let id = store
        .enqueue(
            "s@example.com",
            "r@127.0.0.1",
            "127.0.0.1",
            b"body\r\n",
            None,
            0,
            false,
        )
        .await
        .unwrap();

    let worker = DeliveryWorker::new(
        WorkerConfig {
            // long enough that only the notification can start a poll
            poll_interval_secs: 3600,
            batch_size: 10,
            max_attempts: 8,
            max_concurrent_domains: 2,
            max_messages_per_connection: 5,
        },
        store.clone(),
        notifier.clone(),
        resolver(),
        "client.test".to_string(),
    );
    let (tx, rx) = tokio::sync::watch::channel(false);
    let run = tokio::spawn(async move { worker.run(rx).await });

    notifier.notify().await;
    let attempted = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let m = store.get_message(id).await.unwrap().unwrap();
            if m.attempts > 0 || m.status == QueueStatus::Delivered {
                return m;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the worker woke on the notification and attempted the row");
    assert_eq!(
        attempted.status,
        QueueStatus::Pending,
        "failed attempt is rescheduled"
    );
    assert_eq!(attempted.last_error.as_deref(), Some("all MX hosts failed"));

    tx.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(5), run)
        .await
        .expect("worker stops on shutdown")
        .unwrap();
}
