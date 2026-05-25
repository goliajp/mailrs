//! Postgres / Redis-backed implementations of [`QueueStore`] and [`Notifier`].
//!
//! Enabled by the `pg` feature (on by default). The SQL schema this store
//! targets is published in the [mailrs] repo (`scripts/init-schema.sql` —
//! tables `outbound_queue` and `suppression_list`).
//!
//! [mailrs]: https://github.com/goliajp/mailrs

use redis::AsyncCommands;
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

/// Redis pub/sub-backed [`Notifier`] using the `queue:notify` channel.
#[derive(Debug, Clone)]
pub struct RedisNotifier {
    url: String,
}

impl RedisNotifier {
    /// Build a notifier from a connection URL (e.g.
    /// `redis://127.0.0.1:6379`). Subscribers run on a background tokio task
    /// that this struct spawns when [`wait`](Notifier::wait) is first called.
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }
}

#[async_trait::async_trait]
impl Notifier for RedisNotifier {
    async fn notify(&self) {
        let Ok(client) = redis::Client::open(self.url.as_str()) else {
            return;
        };
        let Ok(mut conn) = client.get_connection_manager().await else {
            return;
        };
        let _: Result<i32, _> = conn.publish("queue:notify", "1").await;
    }

    async fn wait(&self) {
        // Each call opens a fresh pubsub connection. Callers typically await
        // wait() inside a select! arm with a poll-interval sleep, so a short
        // connection lifetime is fine — even if pubsub setup fails or returns
        // immediately, the next poll loop will start a new wait().
        let Ok(client) = redis::Client::open(self.url.as_str()) else {
            std::future::pending::<()>().await;
            return;
        };
        let Ok(mut pubsub) = client.get_async_pubsub().await else {
            std::future::pending::<()>().await;
            return;
        };
        if pubsub.subscribe("queue:notify").await.is_err() {
            std::future::pending::<()>().await;
            return;
        }
        use futures_util::StreamExt;
        let mut stream = pubsub.on_message();
        let _ = stream.next().await;
    }
}
