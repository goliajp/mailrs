use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use tokio::sync::broadcast;

static CONNECTION_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn next_connection_id() -> u64 {
    CONNECTION_COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum SmtpEvent {
    ConnectionOpened {
        id: u64,
        addr: String,
        tls: bool,
    },
    CommandReceived {
        id: u64,
        command: String,
        state_before: String,
    },
    ResponseSent {
        id: u64,
        response: String,
        state_after: String,
    },
    TlsUpgraded {
        id: u64,
    },
    Authenticated {
        id: u64,
        username: String,
    },
    MessageDelivered {
        id: u64,
        from: String,
        to: Vec<String>,
        size: usize,
    },
    SpamRejected {
        id: u64,
        reason: String,
    },
    MessageQueued {
        id: u64,
        from: String,
        to: Vec<String>,
    },
    ConnectionClosed {
        id: u64,
    },
    DeliveryAttempt {
        queue_id: i64,
        domain: String,
    },
    DeliverySuccess {
        queue_id: i64,
        domain: String,
    },
    DeliveryFailed {
        queue_id: i64,
        domain: String,
        error: String,
    },
    BounceGenerated {
        queue_id: i64,
        sender: String,
    },
    NewMessage {
        user: String,
        thread_id: String,
        sender: String,
        subject: String,
        snippet: String,
    },
    /// Inbound message carried a `text/calendar` MIME part and was parsed
    /// successfully by `mailrs::ical`. Web client / macapp use this to
    /// open or refresh an invite-card view in real time.
    InviteReceived {
        user: String,
        message_id: i64,
        method: String,
        uid: String,
    },
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<SmtpEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn emit(&self, event: SmtpEvent) {
        // ignore send errors (no subscribers)
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SmtpEvent> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
#[path = "event_bus_tests.rs"]
mod tests;
