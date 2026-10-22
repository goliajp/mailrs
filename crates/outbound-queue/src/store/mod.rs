//! Storage abstractions for the outbound queue.
//!
//! The bundled [`DeliveryWorker`](crate::DeliveryWorker) talks to the queue
//! only through [`QueueStore`] and is woken through [`Notifier`]. Implement
//! both against your own backend (SQL, a key-value store, a REST service)
//! and hand them to the worker; [`check_queue_store`](crate::store::contract::check_queue_store) checks an
//! implementation against the semantics the worker relies on.
//!
//! [`InMemoryQueueStore`] and [`InMemoryNotifier`] are complete reference
//! implementations for tests and single-process deployments.

pub mod contract;
mod memory;

pub use memory::{InMemoryNotifier, InMemoryQueueStore};

use crate::queue::QueuedMessage;

/// How long an `inflight` row may go without an update before
/// [`QueueStore::recover_stale_inflight`] hands it back to `pending`.
pub const STALE_INFLIGHT_SECS: i64 = 600;

/// Error returned by store operations. Backend-specific details are stringified
/// to keep the trait dyn-compatible across implementations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Backend-specific error (SQL, network — stringified).
    #[error("store backend: {0}")]
    Backend(String),
}

/// The persistent store the outbound queue lives in.
///
/// `enqueue*` is called by the submission path. `claim_for_delivery` +
/// `mark_*` is called by a delivery worker. `list_recent` / `queue_stats` /
/// `cancel_*` / `retry_message` / `reschedule_pending` is called by admin
/// code.
///
/// Lifecycle: `pending → inflight → {delivered, pending (retry), bounced}`.
/// `claim_for_delivery` must be atomic: two concurrent claims never return
/// the same row, so several workers can share one store.
#[async_trait::async_trait]
pub trait QueueStore: Send + Sync {
    /// Insert a new pending message, eligible from `now`. Returns the
    /// assigned id.
    #[allow(clippy::too_many_arguments)]
    async fn enqueue(
        &self,
        sender: &str,
        recipient: &str,
        domain: &str,
        message_data: &[u8],
        message_id: Option<&str>,
        now: i64,
        is_forwarded: bool,
    ) -> Result<i64, StoreError>;

    /// Insert a pending message that becomes eligible at `scheduled_at`.
    #[allow(clippy::too_many_arguments)]
    async fn enqueue_scheduled(
        &self,
        sender: &str,
        recipient: &str,
        domain: &str,
        message_data: &[u8],
        message_id: Option<&str>,
        created_at: i64,
        scheduled_at: i64,
    ) -> Result<i64, StoreError>;

    /// Atomically move up to `limit` pending messages whose `next_retry` is
    /// `<= now` to `inflight` (with `updated_at = now`), earliest
    /// `next_retry` first, and return them as claimed.
    async fn claim_for_delivery(
        &self,
        now: i64,
        limit: u32,
    ) -> Result<Vec<QueuedMessage>, StoreError>;

    /// Reset `inflight` messages not updated for [`STALE_INFLIGHT_SECS`]
    /// back to `pending` (a worker died mid-delivery). Returns rows affected.
    async fn recover_stale_inflight(&self, now: i64) -> Result<u64, StoreError>;

    /// Mark a message as delivered.
    async fn mark_delivered(&self, id: i64, now: i64) -> Result<(), StoreError>;

    /// Record a failed attempt: the message goes back to `pending` with
    /// `attempts + 1`, `last_error = error` and the given `next_retry`.
    async fn mark_failed(
        &self,
        id: i64,
        error: &str,
        next_retry: i64,
        now: i64,
    ) -> Result<(), StoreError>;

    /// Mark a message as permanently bounced (no more retries).
    async fn mark_bounced(&self, id: i64, error: &str, now: i64) -> Result<(), StoreError>;

    /// Fetch a single message by id, or `None` if it does not exist.
    async fn get_message(&self, id: i64) -> Result<Option<QueuedMessage>, StoreError>;

    /// Counts per status that has at least one row — e.g.
    /// `[("pending", 12), ("inflight", 2)]`, in any order.
    async fn queue_stats(&self) -> Result<Vec<(String, i64)>, StoreError>;

    /// Most recently created entries, newest first.
    async fn list_recent(&self, limit: i32) -> Result<Vec<QueuedMessage>, StoreError>;

    /// Delete a single `pending` message. Returns `true` if a row was removed.
    async fn cancel_pending(&self, id: i64) -> Result<bool, StoreError>;

    /// Delete a `pending` message identified by RFC 5322 Message-ID and
    /// sender (so a user can only cancel their own undelivered messages).
    async fn cancel_pending_by_message_id(
        &self,
        message_id: &str,
        sender: &str,
    ) -> Result<bool, StoreError>;

    /// Reset a `bounced` / `failed` message to `pending`, eligible from
    /// `now`. Returns `true` if the message was in one of those states.
    async fn retry_message(&self, id: i64, now: i64) -> Result<bool, StoreError>;

    /// Move a still-`pending` message's delivery time to `scheduled_at`.
    /// Returns `false` when the message is not pending (already claimed,
    /// delivered or gone).
    async fn reschedule_pending(
        &self,
        id: i64,
        scheduled_at: i64,
        now: i64,
    ) -> Result<bool, StoreError>;

    /// `true` if the recipient is on the hard-bounce suppression list.
    async fn is_suppressed(&self, email: &str) -> Result<bool, StoreError>;

    /// Add a recipient to the suppression list, or replace the reason and
    /// code of an existing entry.
    async fn add_suppression(
        &self,
        email: &str,
        reason: &str,
        smtp_code: Option<i32>,
    ) -> Result<(), StoreError>;

    /// Remove a recipient from the suppression list. Returns `true` if a row
    /// was removed.
    async fn remove_suppression(&self, email: &str) -> Result<bool, StoreError>;

    /// `(email, reason, smtp_code, created_at_epoch)` tuples, newest first.
    async fn list_suppressions(
        &self,
        limit: i64,
    ) -> Result<Vec<(String, String, Option<i32>, i64)>, StoreError>;
}

/// Cross-process wake-up channel. The submitter calls [`notify`](Self::notify)
/// after `enqueue`; the worker awaits [`wait`](Self::wait) to cut its poll
/// interval short and pick up new work immediately.
///
/// Implementations may coalesce notifications: many `notify()` calls may
/// release a single `wait()`. The worker re-polls the store on every wake
/// and on its poll interval, so a missed notification costs latency, never
/// work.
#[async_trait::async_trait]
pub trait Notifier: Send + Sync {
    /// Signal that new work was added.
    async fn notify(&self);

    /// Wait until a notification arrives. Spurious wake-ups are allowed.
    async fn wait(&self);
}

/// Notifier that drops every signal. The worker still polls on its configured
/// interval, just without fast wake-ups.
pub struct NoopNotifier;

#[async_trait::async_trait]
impl Notifier for NoopNotifier {
    async fn notify(&self) {}
    async fn wait(&self) {
        std::future::pending::<()>().await;
    }
}
