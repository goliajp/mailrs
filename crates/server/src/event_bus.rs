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
mod tests {
    use super::*;

    #[test]
    fn next_connection_id_is_monotonic() {
        let a = next_connection_id();
        let b = next_connection_id();
        let c = next_connection_id();
        assert!(b > a);
        assert!(c > b);
    }

    #[test]
    fn event_bus_no_subscriber_does_not_panic() {
        let bus = EventBus::new(16);
        bus.emit(SmtpEvent::ConnectionClosed { id: 1 });
    }

    #[tokio::test]
    async fn event_bus_subscriber_receives_event() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();
        bus.emit(SmtpEvent::ConnectionClosed { id: 42 });
        let event = rx.recv().await.unwrap();
        assert!(matches!(event, SmtpEvent::ConnectionClosed { id: 42 }));
    }

    #[tokio::test]
    async fn event_bus_multiple_subscribers() {
        let bus = EventBus::new(16);
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();
        bus.emit(SmtpEvent::TlsUpgraded { id: 1 });
        assert!(matches!(
            rx1.recv().await.unwrap(),
            SmtpEvent::TlsUpgraded { id: 1 }
        ));
        assert!(matches!(
            rx2.recv().await.unwrap(),
            SmtpEvent::TlsUpgraded { id: 1 }
        ));
    }

    #[test]
    fn smtp_event_serializes_to_json() {
        let event = SmtpEvent::MessageDelivered {
            id: 1,
            from: "a@b.c".into(),
            to: vec!["d@e.f".into()],
            size: 1024,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"MessageDelivered\""));
        assert!(json.contains("\"size\":1024"));
    }

    #[test]
    fn all_event_variants_serialize() {
        let events: Vec<SmtpEvent> = vec![
            SmtpEvent::ConnectionOpened {
                id: 0,
                addr: "1.2.3.4".into(),
                tls: false,
            },
            SmtpEvent::CommandReceived {
                id: 0,
                command: "EHLO".into(),
                state_before: "Connected".into(),
            },
            SmtpEvent::ResponseSent {
                id: 0,
                response: "250 OK".into(),
                state_after: "Greeted".into(),
            },
            SmtpEvent::TlsUpgraded { id: 0 },
            SmtpEvent::Authenticated {
                id: 0,
                username: "user".into(),
            },
            SmtpEvent::MessageDelivered {
                id: 0,
                from: "a@b".into(),
                to: vec![],
                size: 0,
            },
            SmtpEvent::SpamRejected {
                id: 0,
                reason: "spam".into(),
            },
            SmtpEvent::MessageQueued {
                id: 0,
                from: "a@b".into(),
                to: vec![],
            },
            SmtpEvent::ConnectionClosed { id: 0 },
            SmtpEvent::DeliveryAttempt {
                queue_id: 0,
                domain: "d".into(),
            },
            SmtpEvent::DeliverySuccess {
                queue_id: 0,
                domain: "d".into(),
            },
            SmtpEvent::DeliveryFailed {
                queue_id: 0,
                domain: "d".into(),
                error: "e".into(),
            },
            SmtpEvent::BounceGenerated {
                queue_id: 0,
                sender: "a@b".into(),
            },
            SmtpEvent::NewMessage {
                user: "u".into(),
                thread_id: "t".into(),
                sender: "s".into(),
                subject: "sub".into(),
                snippet: "sn".into(),
            },
        ];
        for event in events {
            let json = serde_json::to_string(&event);
            assert!(json.is_ok(), "failed to serialize: {event:?}");
        }
    }
}
