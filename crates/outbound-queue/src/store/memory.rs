//! In-process reference implementations of [`QueueStore`] and [`Notifier`].

use std::collections::HashMap;
use std::sync::Mutex;

use super::{Notifier, QueueStore, STALE_INFLIGHT_SECS, StoreError};
use crate::queue::{QueueStatus, QueuedMessage};

/// In-process notifier backed by [`tokio::sync::Notify`].
///
/// Useful for tests and single-process deployments where the worker and
/// submitter share a runtime.
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

/// In-memory [`QueueStore`] for tests and single-process deployments. Not
/// durable across restarts.
pub struct InMemoryQueueStore {
    state: Mutex<MemState>,
}

struct MemState {
    next_id: i64,
    messages: Vec<QueuedMessage>,
    // (email, reason, code, created_at)
    suppressions: Vec<(String, String, Option<i32>, i64)>,
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

    fn update(&self, id: i64, f: impl FnOnce(&mut QueuedMessage)) {
        let mut s = self.state.lock().unwrap();
        if let Some(m) = s.messages.iter_mut().find(|m| m.id == id) {
            f(m);
        }
    }

    fn remove_pending(&self, matches: impl Fn(&QueuedMessage) -> bool) -> bool {
        let mut s = self.state.lock().unwrap();
        let before = s.messages.len();
        s.messages
            .retain(|m| !(m.status == QueueStatus::Pending && matches(m)));
        s.messages.len() < before
    }
}

struct NewMessage<'a> {
    sender: &'a str,
    recipient: &'a str,
    domain: &'a str,
    message_data: &'a [u8],
    message_id: Option<&'a str>,
    next_retry: i64,
    created_at: i64,
    is_forwarded: bool,
}

fn insert(s: &mut MemState, n: NewMessage<'_>) -> i64 {
    let id = s.next_id;
    s.next_id += 1;
    s.messages.push(QueuedMessage {
        id,
        sender: n.sender.into(),
        recipient: n.recipient.into(),
        domain: n.domain.into(),
        message_data: n.message_data.to_vec(),
        status: QueueStatus::Pending,
        attempts: 0,
        max_attempts: 8,
        next_retry: n.next_retry,
        last_error: None,
        message_id: n.message_id.map(str::to_owned),
        created_at: n.created_at,
        updated_at: n.created_at,
        is_forwarded: n.is_forwarded,
    });
    id
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
        Ok(insert(
            &mut s,
            NewMessage {
                sender,
                recipient,
                domain,
                message_data,
                message_id,
                next_retry: now,
                created_at: now,
                is_forwarded,
            },
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
        Ok(insert(
            &mut s,
            NewMessage {
                sender,
                recipient,
                domain,
                message_data,
                message_id,
                next_retry: scheduled_at,
                created_at,
                is_forwarded: false,
            },
        ))
    }

    async fn claim_for_delivery(
        &self,
        now: i64,
        limit: u32,
    ) -> Result<Vec<QueuedMessage>, StoreError> {
        let mut s = self.state.lock().unwrap();
        let mut due: Vec<&mut QueuedMessage> = s
            .messages
            .iter_mut()
            .filter(|m| m.status == QueueStatus::Pending && m.next_retry <= now)
            .collect();
        due.sort_by_key(|m| m.next_retry);
        Ok(due
            .into_iter()
            .take(limit as usize)
            .map(|m| {
                m.status = QueueStatus::InFlight;
                m.updated_at = now;
                m.clone()
            })
            .collect())
    }

    async fn recover_stale_inflight(&self, now: i64) -> Result<u64, StoreError> {
        let threshold = now - STALE_INFLIGHT_SECS;
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

    async fn mark_delivered(&self, id: i64, now: i64) -> Result<(), StoreError> {
        self.update(id, |m| {
            m.status = QueueStatus::Delivered;
            m.updated_at = now;
        });
        Ok(())
    }

    async fn mark_failed(
        &self,
        id: i64,
        error: &str,
        next_retry: i64,
        now: i64,
    ) -> Result<(), StoreError> {
        self.update(id, |m| {
            m.status = QueueStatus::Pending;
            m.attempts += 1;
            m.last_error = Some(error.into());
            m.next_retry = next_retry;
            m.updated_at = now;
        });
        Ok(())
    }

    async fn mark_bounced(&self, id: i64, error: &str, now: i64) -> Result<(), StoreError> {
        self.update(id, |m| {
            m.status = QueueStatus::Bounced;
            m.last_error = Some(error.into());
            m.updated_at = now;
        });
        Ok(())
    }

    async fn get_message(&self, id: i64) -> Result<Option<QueuedMessage>, StoreError> {
        let s = self.state.lock().unwrap();
        Ok(s.messages.iter().find(|m| m.id == id).cloned())
    }

    async fn queue_stats(&self) -> Result<Vec<(String, i64)>, StoreError> {
        let s = self.state.lock().unwrap();
        let mut counts: HashMap<&'static str, i64> = HashMap::new();
        for m in &s.messages {
            *counts.entry(m.status.as_str()).or_insert(0) += 1;
        }
        Ok(counts
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect())
    }

    async fn list_recent(&self, limit: i32) -> Result<Vec<QueuedMessage>, StoreError> {
        let s = self.state.lock().unwrap();
        let mut out: Vec<QueuedMessage> = s.messages.clone();
        out.sort_by_key(|m| std::cmp::Reverse((m.created_at, m.id)));
        out.truncate(limit.max(0) as usize);
        Ok(out)
    }

    async fn cancel_pending(&self, id: i64) -> Result<bool, StoreError> {
        Ok(self.remove_pending(|m| m.id == id))
    }

    async fn cancel_pending_by_message_id(
        &self,
        message_id: &str,
        sender: &str,
    ) -> Result<bool, StoreError> {
        Ok(self
            .remove_pending(|m| m.sender == sender && m.message_id.as_deref() == Some(message_id)))
    }

    async fn retry_message(&self, id: i64, now: i64) -> Result<bool, StoreError> {
        let mut s = self.state.lock().unwrap();
        let Some(m) = s
            .messages
            .iter_mut()
            .find(|m| m.id == id && matches!(m.status, QueueStatus::Bounced | QueueStatus::Failed))
        else {
            return Ok(false);
        };
        m.status = QueueStatus::Pending;
        m.next_retry = now;
        m.updated_at = now;
        Ok(true)
    }

    async fn reschedule_pending(
        &self,
        id: i64,
        scheduled_at: i64,
        now: i64,
    ) -> Result<bool, StoreError> {
        let mut s = self.state.lock().unwrap();
        let Some(m) = s
            .messages
            .iter_mut()
            .find(|m| m.id == id && m.status == QueueStatus::Pending)
        else {
            return Ok(false);
        };
        m.next_retry = scheduled_at;
        m.updated_at = now;
        Ok(true)
    }

    async fn is_suppressed(&self, email: &str) -> Result<bool, StoreError> {
        let s = self.state.lock().unwrap();
        Ok(s.suppressions.iter().any(|(e, _, _, _)| e == email))
    }

    async fn add_suppression(
        &self,
        email: &str,
        reason: &str,
        smtp_code: Option<i32>,
    ) -> Result<(), StoreError> {
        let mut s = self.state.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        s.suppressions.retain(|(e, _, _, _)| e != email);
        s.suppressions
            .push((email.into(), reason.into(), smtp_code, now));
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
        // pushed in insertion order, so newest is last even within one second
        let mut out: Vec<_> = s.suppressions.iter().rev().cloned().collect();
        out.truncate(limit.max(0) as usize);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn in_memory_store_meets_the_contract() {
        super::super::contract::check_queue_store(|| async { InMemoryQueueStore::new() }).await;
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
