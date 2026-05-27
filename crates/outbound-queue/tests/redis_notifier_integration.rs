//! `RedisNotifier` integration tests against an ephemeral Valkey container.
//!
//! Covers the `Notifier` trait surface on `RedisNotifier` (pubsub
//! publish + subscribe wakeup, error handling on bad URLs).

mod common;

use std::time::Duration;

use common::redis::start_redis;
use mailrs_outbound_queue::pg_store::RedisNotifier;
use mailrs_outbound_queue::store::Notifier;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn notifier_publish_wakes_subscriber() {
    let (_container, url) = start_redis().await;
    let publisher = RedisNotifier::new(url.clone());
    let subscriber = RedisNotifier::new(url);

    // Spawn the subscribe first so it's listening when the publish lands.
    let h = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(5), subscriber.wait())
            .await
            .expect("notifier wait timed out");
    });

    // Give the subscriber a moment to connect + subscribe before publishing.
    tokio::time::sleep(Duration::from_millis(250)).await;
    publisher.notify().await;
    h.await.expect("subscriber task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn notifier_clone_yields_same_target() {
    let (_container, url) = start_redis().await;
    let n = RedisNotifier::new(url);
    let cloned = n.clone();
    let h = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(5), cloned.wait())
            .await
            .expect("cloned subscriber timed out");
    });
    tokio::time::sleep(Duration::from_millis(250)).await;
    n.notify().await;
    h.await.expect("cloned subscriber task");
}
