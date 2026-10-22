//! `deliver_domain_static` — what it marks delivered, retried, bounced
//! or skipped.

use common::mock_smtp::{Behavior, ensure_crypto_provider, spawn_mock_smtp};
use mailrs_outbound_queue::queue::QueueStatus;
use mailrs_outbound_queue::worker::deliver_domain_static;
use mailrs_outbound_queue::{InMemoryQueueStore, QueueStore};

use super::common;
use super::resolver;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deliver_domain_static_happy_marks_delivered() {
    ensure_crypto_provider();
    let mock = spawn_mock_smtp(Behavior::AcceptNoStarttls).await;
    let store = InMemoryQueueStore::new();

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
    let claimed = store.claim_for_delivery(0, 10).await.unwrap();
    assert_eq!(claimed.len(), 1);

    let r = resolver();
    // domain "127.0.0.1" is treated as an MX-less destination by
    // mailrs_smtp_client::resolve_mx — it falls back to using the
    // domain itself as the exchange, which sends the client to
    // 127.0.0.1:<mock-port>.
    deliver_domain_static(
        &r,
        "client.test",
        "127.0.0.1",
        claimed,
        &store,
        mock.addr.port(),
        50,
        None,
    )
    .await;

    let after = store.get_message(id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        QueueStatus::Delivered,
        "happy path marks delivered"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deliver_domain_static_5xx_marks_failed_for_retry() {
    ensure_crypto_provider();
    let mock = spawn_mock_smtp(Behavior::Reject5xxAfterRcpt).await;
    let store = InMemoryQueueStore::new();

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
    let claimed = store.claim_for_delivery(0, 10).await.unwrap();

    let r = resolver();
    deliver_domain_static(
        &r,
        "client.test",
        "127.0.0.1",
        claimed,
        &store,
        mock.addr.port(),
        50,
        None,
    )
    .await;

    let after = store.get_message(id).await.unwrap().unwrap();
    // With max_attempts=8 and attempts=0 going in, a single 5xx
    // does NOT bounce — it transitions through mark_failed back to
    // pending with attempts=1. (Bounce-or-retry is decided by
    // should_bounce(attempts+1, max_attempts).)
    assert_eq!(after.status, QueueStatus::Pending);
    assert_eq!(after.attempts, 1);
    assert!(after.last_error.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deliver_domain_static_5xx_at_max_attempts_bounces() {
    ensure_crypto_provider();
    let mock = spawn_mock_smtp(Behavior::Reject5xxAfterMail).await;
    let store = InMemoryQueueStore::new();

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
    let mut claimed = store.claim_for_delivery(0, 10).await.unwrap();
    // the worker decides bounce-or-retry from the claimed row's attempts
    claimed[0].attempts = claimed[0].max_attempts;

    let r = resolver();
    deliver_domain_static(
        &r,
        "client.test",
        "127.0.0.1",
        claimed,
        &store,
        mock.addr.port(),
        50,
        None,
    )
    .await;

    let after = store.get_message(id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        QueueStatus::Bounced,
        "max attempts reached → bounced"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deliver_domain_static_suppressed_recipient_is_skipped() {
    ensure_crypto_provider();
    let mock = spawn_mock_smtp(Behavior::AcceptNoStarttls).await;
    let store = InMemoryQueueStore::new();

    let id = store
        .enqueue(
            "s@example.com",
            "blocked@127.0.0.1",
            "127.0.0.1",
            b"body\r\n",
            None,
            0,
            false,
        )
        .await
        .unwrap();
    store
        .add_suppression("blocked@127.0.0.1", "previous 550", Some(550))
        .await
        .unwrap();
    let claimed = store.claim_for_delivery(0, 10).await.unwrap();

    let r = resolver();
    deliver_domain_static(
        &r,
        "client.test",
        "127.0.0.1",
        claimed,
        &store,
        mock.addr.port(),
        50,
        None,
    )
    .await;

    let after = store.get_message(id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        QueueStatus::Bounced,
        "suppressed recipient is bounced before MX resolve"
    );
    assert!(after.last_error.as_deref().unwrap().contains("suppressed"));
}
