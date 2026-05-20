//! Storage abstractions for the outbound queue.
//!
//! The bundled [`DeliveryWorker`](crate::DeliveryWorker) is currently
//! Postgres-only (enabled with the `pg` feature). This module defines the
//! traits a v1.x-compatible custom worker would target. External users who
//! don't want sqlx can disable the `pg` feature, implement [`QueueStore`]
//! and [`Notifier`] against their own backend (sqlite, sled, custom REST,
//! in-memory for tests), and reuse the pure delivery primitives in this
//! crate (`dkim_sign`, `dsn`, `mta_sts`, `retry`) plus
//! [`mailrs-smtp-client`](https://crates.io/crates/mailrs-smtp-client).
//!
//! Wider trait coverage and a generic worker are planned for a future minor
//! release.

use std::sync::Mutex;

use crate::queue::{QueueStatus, QueuedMessage};

/// Error returned by store operations. Backend-specific details are stringified
/// to keep the trait dyn-compatible across implementations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Backend-specific error (PG, SQLite, network — stringified).
    #[error("store backend: {0}")]
    Backend(String),
}

#[cfg(feature = "pg")]
impl From<sqlx::Error> for StoreError {
    fn from(err: sqlx::Error) -> Self {
        Self::Backend(err.to_string())
    }
}

/// The persistent store the outbound queue lives in.
///
/// `enqueue*` is called by the SMTP submission path. `dequeue` + `mark_*` is
/// called by a delivery worker. `list_recent` / `queue_stats` /
/// `cancel_*` / `retry_message` is called by admin / management code.
///
/// Implementations should be ACID-safe for the lifecycle transitions
/// `pending → inflight → {delivered, failed, bounced}` so a crashed worker
/// can't lose or duplicate-deliver a message.
#[async_trait::async_trait]
pub trait QueueStore: Send + Sync {
    /// Insert a new pending message. Returns the assigned id.
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

    /// Insert a message scheduled to deliver at `scheduled_at`.
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

    /// Fetch up to `limit` pending messages whose `next_retry` is `<= now`.
    async fn dequeue(&self, now: i64, limit: u32) -> Result<Vec<QueuedMessage>, StoreError>;

    /// Reset `inflight` messages older than ~10 minutes back to `pending`
    /// (crash-recovery). Returns rows affected.
    async fn recover_stale_inflight(&self, now: i64) -> Result<u64, StoreError>;

    /// Mark a message as currently being delivered.
    async fn mark_inflight(&self, id: i64, now: i64) -> Result<(), StoreError>;

    /// Mark a message as delivered.
    async fn mark_delivered(&self, id: i64, now: i64) -> Result<(), StoreError>;

    /// Mark a delivery attempt as failed; the message goes back to `pending`
    /// with an incremented `attempts` and a `next_retry` of `next_retry`.
    async fn mark_failed(
        &self,
        id: i64,
        error: &str,
        next_retry: i64,
        now: i64,
    ) -> Result<(), StoreError>;

    /// Mark a message as permanently bounced (no more retries).
    async fn mark_bounced(&self, id: i64, error: &str, now: i64) -> Result<(), StoreError>;

    /// Fetch a single message by id, or `None` if it has been purged.
    async fn get_message(&self, id: i64) -> Result<Option<QueuedMessage>, StoreError>;

    /// Group counts by status — e.g. `[("pending", 12), ("inflight", 2)]`.
    async fn queue_stats(&self) -> Result<Vec<(String, i64)>, StoreError>;

    /// Recently-created entries, newest first, for admin UIs.
    async fn list_recent(&self, limit: i32) -> Result<Vec<QueuedMessage>, StoreError>;

    /// Delete a single `pending` message. Returns `true` if a row was removed.
    async fn cancel_pending(&self, id: i64) -> Result<bool, StoreError>;

    /// Delete a single `pending` message identified by RFC 5322 Message-ID and
    /// sender (so a user can only cancel their own undelivered messages).
    async fn cancel_pending_by_message_id(
        &self,
        message_id: &str,
        sender: &str,
    ) -> Result<bool, StoreError>;

    /// Reset a `bounced` / `failed` message to `pending` for another try.
    async fn retry_message(&self, id: i64, now: i64) -> Result<bool, StoreError>;

    /// `true` if the recipient is in the hard-bounce suppression list.
    async fn is_suppressed(&self, email: &str) -> bool;

    /// Add (or update) a recipient on the suppression list.
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
/// after `enqueue`; the worker awaits [`wait`](Self::wait) to short-circuit
/// its poll interval and pick up new work immediately.
///
/// Implementations may coalesce notifications: many `notify()` calls may
/// release a single `wait()`. The worker still re-polls the store on every
/// wake, so missing a notification is at worst a latency cost, never lost
/// work.
#[async_trait::async_trait]
pub trait Notifier: Send + Sync {
    /// Signal that new work was added.
    async fn notify(&self);

    /// Block until a notification arrives. Spurious wake-ups are allowed.
    async fn wait(&self);
}

/// In-process notifier backed by [`tokio::sync::Notify`].
///
/// Useful for tests, single-process deployments where the worker and
/// submitter share a runtime, or as a fallback when the cross-process
/// notifier is unavailable.
#[derive(Default)]
pub struct InMemoryNotifier {
    inner: tokio::sync::Notify,
}

impl InMemoryNotifier {
    /// Create a new notifier with no waiters.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl Notifier for InMemoryNotifier {
    async fn notify(&self) {
        self.inner.notify_one();
    }
    async fn wait(&self) {
        self.inner.notified().await;
    }
}

/// Notifier that drops every signal. The worker still polls on its configured
/// interval, just without fast wake-ups. Useful when a real notifier hasn't
/// been wired up yet.
pub struct NoopNotifier;

#[async_trait::async_trait]
impl Notifier for NoopNotifier {
    async fn notify(&self) {}
    async fn wait(&self) {
        std::future::pending::<()>().await;
    }
}

/// Pure in-memory [`QueueStore`] for tests and single-process pilot
/// deployments. Not durable across restarts.
pub struct InMemoryQueueStore {
    state: Mutex<MemState>,
}

struct MemState {
    next_id: i64,
    messages: Vec<QueuedMessage>,
    suppressions: Vec<(String, String, Option<i32>, i64)>, // (email, reason, code, created_at)
}

impl Default for InMemoryQueueStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryQueueStore {
    /// Create an empty in-memory store.
    pub fn new() -> Self {
        Self {
            state: Mutex::new(MemState {
                next_id: 1,
                messages: Vec::new(),
                suppressions: Vec::new(),
            }),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_msg(
        s: &mut MemState,
        sender: &str,
        recipient: &str,
        domain: &str,
        message_data: &[u8],
        message_id: Option<&str>,
        next_retry: i64,
        created_at: i64,
        is_forwarded: bool,
    ) -> i64 {
        let id = s.next_id;
        s.next_id += 1;
        s.messages.push(QueuedMessage {
            id,
            sender: sender.into(),
            recipient: recipient.into(),
            domain: domain.into(),
            message_data: message_data.to_vec(),
            status: QueueStatus::Pending,
            attempts: 0,
            max_attempts: 8,
            next_retry,
            last_error: None,
            message_id: message_id.map(str::to_owned),
            created_at,
            updated_at: created_at,
            is_forwarded,
        });
        id
    }
}

#[async_trait::async_trait]
impl QueueStore for InMemoryQueueStore {
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
        let mut s = self.state.lock().unwrap();
        Ok(Self::insert_msg(
            &mut s, sender, recipient, domain, message_data, message_id, now, now, is_forwarded,
        ))
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
        let mut s = self.state.lock().unwrap();
        Ok(Self::insert_msg(
            &mut s,
            sender,
            recipient,
            domain,
            message_data,
            message_id,
            scheduled_at,
            created_at,
            false,
        ))
    }

    async fn dequeue(&self, now: i64, limit: u32) -> Result<Vec<QueuedMessage>, StoreError> {
        let s = self.state.lock().unwrap();
        let mut out: Vec<QueuedMessage> = s
            .messages
            .iter()
            .filter(|m| m.status == QueueStatus::Pending && m.next_retry <= now)
            .cloned()
            .collect();
        out.sort_by_key(|m| m.next_retry);
        out.truncate(limit as usize);
        Ok(out)
    }

    async fn recover_stale_inflight(&self, now: i64) -> Result<u64, StoreError> {
        let threshold = now - 600;
        let mut s = self.state.lock().unwrap();
        let mut affected = 0u64;
        for m in s.messages.iter_mut() {
            if m.status == QueueStatus::InFlight && m.updated_at < threshold {
                m.status = QueueStatus::Pending;
                m.updated_at = now;
                affected += 1;
            }
        }
        Ok(affected)
    }

    async fn mark_inflight(&self, id: i64, now: i64) -> Result<(), StoreError> {
        let mut s = self.state.lock().unwrap();
        if let Some(m) = s.messages.iter_mut().find(|m| m.id == id) {
            m.status = QueueStatus::InFlight;
            m.updated_at = now;
        }
        Ok(())
    }

    async fn mark_delivered(&self, id: i64, now: i64) -> Result<(), StoreError> {
        let mut s = self.state.lock().unwrap();
        if let Some(m) = s.messages.iter_mut().find(|m| m.id == id) {
            m.status = QueueStatus::Delivered;
            m.updated_at = now;
        }
        Ok(())
    }

    async fn mark_failed(
        &self,
        id: i64,
        error: &str,
        next_retry: i64,
        now: i64,
    ) -> Result<(), StoreError> {
        let mut s = self.state.lock().unwrap();
        if let Some(m) = s.messages.iter_mut().find(|m| m.id == id) {
            m.status = QueueStatus::Pending;
            m.attempts += 1;
            m.last_error = Some(error.into());
            m.next_retry = next_retry;
            m.updated_at = now;
        }
        Ok(())
    }

    async fn mark_bounced(&self, id: i64, error: &str, now: i64) -> Result<(), StoreError> {
        let mut s = self.state.lock().unwrap();
        if let Some(m) = s.messages.iter_mut().find(|m| m.id == id) {
            m.status = QueueStatus::Bounced;
            m.last_error = Some(error.into());
            m.updated_at = now;
        }
        Ok(())
    }

    async fn get_message(&self, id: i64) -> Result<Option<QueuedMessage>, StoreError> {
        let s = self.state.lock().unwrap();
        Ok(s.messages.iter().find(|m| m.id == id).cloned())
    }

    async fn queue_stats(&self) -> Result<Vec<(String, i64)>, StoreError> {
        use std::collections::HashMap;
        let s = self.state.lock().unwrap();
        let mut counts: HashMap<&'static str, i64> = HashMap::new();
        for m in &s.messages {
            *counts.entry(m.status.as_str()).or_insert(0) += 1;
        }
        Ok(counts.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    async fn list_recent(&self, limit: i32) -> Result<Vec<QueuedMessage>, StoreError> {
        let s = self.state.lock().unwrap();
        let mut out: Vec<QueuedMessage> = s.messages.clone();
        out.sort_by_key(|m| std::cmp::Reverse(m.created_at));
        out.truncate(limit.max(0) as usize);
        Ok(out)
    }

    async fn cancel_pending(&self, id: i64) -> Result<bool, StoreError> {
        let mut s = self.state.lock().unwrap();
        let before = s.messages.len();
        s.messages
            .retain(|m| !(m.id == id && m.status == QueueStatus::Pending));
        Ok(s.messages.len() < before)
    }

    async fn cancel_pending_by_message_id(
        &self,
        message_id: &str,
        sender: &str,
    ) -> Result<bool, StoreError> {
        let mut s = self.state.lock().unwrap();
        let before = s.messages.len();
        s.messages.retain(|m| {
            !(m.status == QueueStatus::Pending
                && m.sender == sender
                && m.message_id.as_deref() == Some(message_id))
        });
        Ok(s.messages.len() < before)
    }

    async fn retry_message(&self, id: i64, now: i64) -> Result<bool, StoreError> {
        let mut s = self.state.lock().unwrap();
        if let Some(m) = s.messages.iter_mut().find(|m| {
            m.id == id && (m.status == QueueStatus::Bounced || m.status == QueueStatus::Failed)
        }) {
            m.status = QueueStatus::Pending;
            m.next_retry = now;
            m.updated_at = now;
            return Ok(true);
        }
        Ok(false)
    }

    async fn is_suppressed(&self, email: &str) -> bool {
        let s = self.state.lock().unwrap();
        s.suppressions.iter().any(|(e, _, _, _)| e == email)
    }

    async fn add_suppression(
        &self,
        email: &str,
        reason: &str,
        smtp_code: Option<i32>,
    ) -> Result<(), StoreError> {
        let mut s = self.state.lock().unwrap();
        if let Some(entry) = s.suppressions.iter_mut().find(|(e, _, _, _)| e == email) {
            entry.1 = reason.into();
            entry.2 = smtp_code;
        } else {
            let now = chrono::Utc::now().timestamp();
            s.suppressions
                .push((email.into(), reason.into(), smtp_code, now));
        }
        Ok(())
    }

    async fn remove_suppression(&self, email: &str) -> Result<bool, StoreError> {
        let mut s = self.state.lock().unwrap();
        let before = s.suppressions.len();
        s.suppressions.retain(|(e, _, _, _)| e != email);
        Ok(s.suppressions.len() < before)
    }

    async fn list_suppressions(
        &self,
        limit: i64,
    ) -> Result<Vec<(String, String, Option<i32>, i64)>, StoreError> {
        let s = self.state.lock().unwrap();
        let mut out = s.suppressions.clone();
        out.sort_by_key(|(_, _, _, t)| std::cmp::Reverse(*t));
        out.truncate(limit.max(0) as usize);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    fn store() -> Arc<dyn QueueStore> {
        Arc::new(InMemoryQueueStore::new())
    }

    #[tokio::test]
    async fn enqueue_dequeue_roundtrip() {
        let s = store();
        let id = s
            .enqueue("a@x", "b@y", "y", b"raw", None, 0, false)
            .await
            .unwrap();
        let msgs = s.dequeue(0, 10).await.unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].id, id);
        assert_eq!(msgs[0].sender, "a@x");
    }

    #[tokio::test]
    async fn lifecycle_transitions() {
        let s = store();
        let id = s.enqueue("a@x", "b@y", "y", b"", None, 0, false).await.unwrap();
        s.mark_inflight(id, 10).await.unwrap();
        s.mark_delivered(id, 20).await.unwrap();
        assert_eq!(s.get_message(id).await.unwrap().unwrap().status, QueueStatus::Delivered);
    }

    #[tokio::test]
    async fn stale_inflight_recovers() {
        let s = store();
        let id = s.enqueue("a@x", "b@y", "y", b"", None, 0, false).await.unwrap();
        s.mark_inflight(id, 0).await.unwrap();
        // 10 minutes + 1 second later, recover_stale should kick it back
        let n = s.recover_stale_inflight(601).await.unwrap();
        assert_eq!(n, 1);
        assert_eq!(s.get_message(id).await.unwrap().unwrap().status, QueueStatus::Pending);
    }

    #[tokio::test]
    async fn suppression_list() {
        let s = store();
        assert!(!s.is_suppressed("user@x").await);
        s.add_suppression("user@x", "bounced", Some(550)).await.unwrap();
        assert!(s.is_suppressed("user@x").await);
        assert!(s.remove_suppression("user@x").await.unwrap());
        assert!(!s.is_suppressed("user@x").await);
    }

    #[tokio::test]
    async fn in_memory_notifier_wakes_waiter() {
        let n = Arc::new(InMemoryNotifier::new());
        let n2 = n.clone();
        let h = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(2), n2.wait())
                .await
                .expect("waiter timed out")
        });
        n.notify().await;
        h.await.unwrap();
    }
}
