//! `deliver_domain_static` — what it marks delivered, retried, bounced
//! or skipped.

use common::mock_smtp::{Behavior, ensure_crypto_provider, spawn_mock_smtp};
use common::pg::start_pg;
use mailrs_outbound_queue::queue::{self, QueueStatus};
use mailrs_outbound_queue::worker::deliver_domain_static;

use super::common;
use super::resolver;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deliver_domain_static_happy_marks_delivered() {
    ensure_crypto_provider();
    let mock = spawn_mock_smtp(Behavior::AcceptNoStarttls).await;
    let (_c, pool) = start_pg().await;

    let id = queue::enqueue_ex(
        &pool,
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
    let claimed = queue::claim_for_delivery(&pool, 0, 10).await.unwrap();
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
        &pool,
        mock.addr.port(),
        50,
        None,
    )
    .await;

    let after = queue::get_message(&pool, id).await.unwrap().unwrap();
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
    let (_c, pool) = start_pg().await;

    let id = queue::enqueue_ex(
        &pool,
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
    let claimed = queue::claim_for_delivery(&pool, 0, 10).await.unwrap();

    let r = resolver();
    deliver_domain_static(
        &r,
        "client.test",
        "127.0.0.1",
        claimed,
        &pool,
        mock.addr.port(),
        50,
        None,
    )
    .await;

    let after = queue::get_message(&pool, id).await.unwrap().unwrap();
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
    let (_c, pool) = start_pg().await;

    let id = queue::enqueue_ex(
        &pool,
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
    // Bump attempts to max_attempts so the next failure bounces.
    sqlx::query("UPDATE outbound_queue SET attempts = max_attempts WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let claimed = queue::claim_for_delivery(&pool, 0, 10).await.unwrap();

    let r = resolver();
    deliver_domain_static(
        &r,
        "client.test",
        "127.0.0.1",
        claimed,
        &pool,
        mock.addr.port(),
        50,
        None,
    )
    .await;

    let after = queue::get_message(&pool, id).await.unwrap().unwrap();
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
    let (_c, pool) = start_pg().await;

    let id = queue::enqueue_ex(
        &pool,
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
    queue::add_suppression(&pool, "blocked@127.0.0.1", "previous 550", Some(550))
        .await
        .unwrap();
    let claimed = queue::claim_for_delivery(&pool, 0, 10).await.unwrap();

    let r = resolver();
    deliver_domain_static(
        &r,
        "client.test",
        "127.0.0.1",
        claimed,
        &pool,
        mock.addr.port(),
        50,
        None,
    )
    .await;

    let after = queue::get_message(&pool, id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        QueueStatus::Bounced,
        "suppressed recipient is bounced before MX resolve"
    );
    assert!(after.last_error.as_deref().unwrap().contains("suppressed"));
}
