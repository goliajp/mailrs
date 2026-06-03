//! `KevyNotifier` integration tests against an in-process `kevy_embedded::Store`.
//!
//! Covers the `Notifier` trait surface on `KevyNotifier` (in-process bus
//! publish + subscribe wakeup, clone sharing the same backing store).

use std::time::Duration;

use kevy_embedded::{Config, Store};
use mailrs_outbound_queue::pg_store::KevyNotifier;
use mailrs_outbound_queue::store::Notifier;

fn make_store() -> Store {
    Store::open(Config::default()).expect("open in-memory store")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn notifier_publish_wakes_subscriber() {
    let store = make_store();
    let publisher = KevyNotifier::new(store.clone());
    let subscriber = KevyNotifier::new(store);

    // Spawn the subscribe first so it's listening when the publish lands.
    let h = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(5), subscriber.wait())
            .await
            .expect("notifier wait timed out");
    });

    // Give the subscriber a moment to subscribe before publishing.
    tokio::time::sleep(Duration::from_millis(50)).await;
    publisher.notify().await;
    h.await.expect("subscriber task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn notifier_clone_yields_same_target() {
    let store = make_store();
    let n = KevyNotifier::new(store);
    let cloned = n.clone();
    let h = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(5), cloned.wait())
            .await
            .expect("cloned subscriber timed out");
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    n.notify().await;
    h.await.expect("cloned subscriber task");
}
