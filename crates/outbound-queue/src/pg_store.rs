//! Postgres / Kevy-backed implementations of [`QueueStore`] and [`Notifier`].
//!
//! Enabled by the `pg` feature (on by default). The SQL schema this store
//! targets is published in the [mailrs] repo (`scripts/init-schema.sql` —
//! tables `outbound_queue` and `suppression_list`).
//!
//! [mailrs]: https://github.com/goliajp/mailrs

use kevy_embedded::{PubsubFrame, Store};
use sqlx::PgPool;

use crate::queue;
use crate::queue::QueuedMessage;
use crate::store::{Notifier, QueueStore, StoreError};

/// `QueueStore` backed by a `sqlx::PgPool` against the mailrs schema.
#[derive(Debug, Clone)]
pub struct PgQueueStore {
    pool: PgPool,
}

impl PgQueueStore {
    /// Build a Postgres-backed store over an existing connection pool. The
    /// pool is cloned cheaply on every operation, so a single pool can be
    /// shared with the rest of the application.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Borrow the underlying pool for ad-hoc queries.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[async_trait::async_trait]
impl QueueStore for PgQueueStore {
    async fn enqueue(
        &self,
        sender: &str,
        recipient: &str,
        domain: &str,
        message_data: &[u8],
        message_id: Option<&str>,
        now: i64,
        is_forwarded: bool,
    ) -> Result<i64, StoreError> {
        Ok(queue::enqueue_ex(
            &self.pool,
            sender,
            recipient,
            domain,
            message_data,
            message_id,
            now,
            is_forwarded,
        )
        .await?)
    }

    async fn enqueue_scheduled(
        &self,
        sender: &str,
        recipient: &str,
        domain: &str,
        message_data: &[u8],
        message_id: Option<&str>,
        created_at: i64,
        scheduled_at: i64,
    ) -> Result<i64, StoreError> {
        Ok(queue::enqueue_scheduled(
            &self.pool,
            sender,
            recipient,
            domain,
            message_data,
            message_id,
            created_at,
            scheduled_at,
        )
        .await?)
    }

    async fn dequeue(&self, now: i64, limit: u32) -> Result<Vec<QueuedMessage>, StoreError> {
        Ok(queue::dequeue(&self.pool, now, limit).await?)
    }

    async fn recover_stale_inflight(&self, now: i64) -> Result<u64, StoreError> {
        Ok(queue::recover_stale_inflight(&self.pool, now).await?)
    }

    async fn mark_inflight(&self, id: i64, now: i64) -> Result<(), StoreError> {
        Ok(queue::mark_inflight(&self.pool, id, now).await?)
    }

    async fn mark_delivered(&self, id: i64, now: i64) -> Result<(), StoreError> {
        Ok(queue::mark_delivered(&self.pool, id, now).await?)
    }

    async fn mark_failed(
        &self,
        id: i64,
        error: &str,
        next_retry: i64,
        now: i64,
    ) -> Result<(), StoreError> {
        Ok(queue::mark_failed(&self.pool, id, error, next_retry, now).await?)
    }

    async fn mark_bounced(&self, id: i64, error: &str, now: i64) -> Result<(), StoreError> {
        Ok(queue::mark_bounced(&self.pool, id, error, now).await?)
    }

    async fn get_message(&self, id: i64) -> Result<Option<QueuedMessage>, StoreError> {
        Ok(queue::get_message(&self.pool, id).await?)
    }

    async fn queue_stats(&self) -> Result<Vec<(String, i64)>, StoreError> {
        Ok(queue::queue_stats(&self.pool).await?)
    }

    async fn list_recent(&self, limit: i32) -> Result<Vec<QueuedMessage>, StoreError> {
        Ok(queue::list_recent(&self.pool, limit).await?)
    }

    async fn cancel_pending(&self, id: i64) -> Result<bool, StoreError> {
        Ok(queue::cancel_pending(&self.pool, id).await?)
    }

    async fn cancel_pending_by_message_id(
        &self,
        message_id: &str,
        sender: &str,
    ) -> Result<bool, StoreError> {
        Ok(queue::cancel_pending_by_message_id(&self.pool, message_id, sender).await?)
    }

    async fn retry_message(&self, id: i64, now: i64) -> Result<bool, StoreError> {
        Ok(queue::retry_message(&self.pool, id, now).await?)
    }

    async fn is_suppressed(&self, email: &str) -> bool {
        queue::is_suppressed(&self.pool, email).await
    }

    async fn add_suppression(
        &self,
        email: &str,
        reason: &str,
        smtp_code: Option<i32>,
    ) -> Result<(), StoreError> {
        Ok(queue::add_suppression(&self.pool, email, reason, smtp_code).await?)
    }

    async fn remove_suppression(&self, email: &str) -> Result<bool, StoreError> {
        Ok(queue::remove_suppression(&self.pool, email).await?)
    }

    async fn list_suppressions(
        &self,
        limit: i64,
    ) -> Result<Vec<(String, String, Option<i32>, i64)>, StoreError> {
        Ok(queue::list_suppressions(&self.pool, limit).await?)
    }
}

/// In-process pub/sub-backed [`Notifier`] using the `queue:notify` channel
/// on an [`kevy_embedded::Store`].
///
/// Producer side publishes synchronously (microsecond) so `notify()` does
/// not block the tokio runtime in any meaningful way. The consumer side
/// (`wait()`) wraps the sync `Subscription::recv` in `spawn_blocking` so
/// the dispatcher stays cooperative; ack frames are filtered out so the
/// caller only sees real publish events.
#[derive(Clone)]
pub struct KevyNotifier {
    store: Store,
}

impl std::fmt::Debug for KevyNotifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KevyNotifier").finish_non_exhaustive()
    }
}

impl KevyNotifier {
    /// Build a notifier on top of an in-process [`Store`]. `Store: Clone`
    /// so callers typically pass a clone of the shared cement-owned store.
    pub fn new(store: Store) -> Self {
        Self { store }
    }
}

#[async_trait::async_trait]
impl Notifier for KevyNotifier {
    async fn notify(&self) {
        let _ = self.store.publish(b"queue:notify", b"1");
    }

    async fn wait(&self) {
        let store = self.store.clone();
        let _ = tokio::task::spawn_blocking(move || {
            // Subscription is !Sync (internal mpsc::Receiver), so it's
            // constructed and consumed entirely inside this blocking task.
            let sub = store.subscribe(&[b"queue:notify"]);
            loop {
                match sub.recv() {
                    Ok(PubsubFrame::Message { .. } | PubsubFrame::Pmessage { .. }) => break,
                    Ok(_) => continue, // Subscribe/Unsubscribe acks — ignore
                    Err(_) => break,   // bus dropped
                }
            }
        })
        .await;
    }
}
