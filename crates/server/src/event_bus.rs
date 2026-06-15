use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

static CONNECTION_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn next_connection_id() -> u64 {
    CONNECTION_COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl SmtpEvent {
    /// Whether this event is worth publishing to a shared kevy-server
    /// for cross-process delivery (receiver-split topology). Only the
    /// user-facing mail events cross; high-frequency protocol-trace
    /// events stay in-process. (P5/P6 may refine which events the
    /// receiver vs core actually publish.)
    pub fn crosses_process(&self) -> bool {
        matches!(
            self,
            SmtpEvent::NewMessage { .. } | SmtpEvent::InviteReceived { .. }
        )
    }
}

/// Envelope wrapping an [`SmtpEvent`] for broadcast.
///
/// Carries an optional pre-serialised JSON cache populated lazily on
/// first call to [`Self::json`]. All subscribers receiving the same
/// `Arc<BroadcastEvent>` share the cache — for N web-socket / JMAP
/// push subscribers consuming the same event, JSON serialisation
/// runs exactly once instead of N times.
pub struct BroadcastEvent {
    /// The typed event. Subscribers that pattern-match (ai_analyzer,
    /// webhook listener, cache invalidator) read this field directly.
    pub event: SmtpEvent,
    json: OnceLock<Arc<str>>,
}

impl BroadcastEvent {
    fn new(event: SmtpEvent) -> Self {
        Self {
            event,
            json: OnceLock::new(),
        }
    }

    /// Get (or lazily compute) the JSON serialisation of the event.
    /// Returns an `Arc<str>` so cloning into per-subscriber send paths
    /// is free.
    pub fn json(&self) -> Arc<str> {
        self.json
            .get_or_init(|| {
                serde_json::to_string(&self.event)
                    .map(Arc::from)
                    .unwrap_or_else(|_| Arc::from(""))
            })
            .clone()
    }
}

/// Cross-process publisher for notify-worthy [`SmtpEvent`]s. Abstracted so
/// the bus doesn't bind the concrete kevy-server publisher — the kevy-backed
/// impl (`KevyEventPublisher`) lives in `kevy_notify`; the receiver-split
/// topology can supply a network publisher without `event_bus` depending on
/// the kevy client.
pub trait EventPublisher: Send + Sync {
    /// Publish `event` to the shared channel (best-effort, fire-and-forget).
    fn publish(&self, event: &SmtpEvent);
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Arc<BroadcastEvent>>,
    /// When set, [`Self::emit`] also publishes cross-process-worthy
    /// events to a shared kevy-server (receiver-split topology).
    publisher: Option<Arc<dyn EventPublisher>>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self {
            tx,
            publisher: None,
        }
    }

    /// Attach a cross-process publisher. Set before the bus is cloned
    /// around so every clone shares it.
    pub fn with_publisher(mut self, publisher: Arc<dyn EventPublisher>) -> Self {
        self.publisher = Some(publisher);
        self
    }

    pub fn emit(&self, event: SmtpEvent) {
        // cross-process publish (best-effort) for notify-worthy events,
        // before the local broadcast so a slow publisher doesn't gate it.
        if let Some(ref publisher) = self.publisher
            && event.crosses_process()
        {
            publisher.publish(&event);
        }
        // ignore send errors (no subscribers)
        let _ = self.tx.send(Arc::new(BroadcastEvent::new(event)));
    }

    /// Broadcast locally only — never re-publishes cross-process. Used
    /// by the kevy subscriber bridge to inject events received from
    /// other processes without looping them back out.
    pub fn emit_local(&self, event: SmtpEvent) {
        let _ = self.tx.send(Arc::new(BroadcastEvent::new(event)));
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<BroadcastEvent>> {
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
        let env = rx.recv().await.unwrap();
        assert!(matches!(env.event, SmtpEvent::ConnectionClosed { id: 42 }));
    }

    #[tokio::test]
    async fn event_bus_multiple_subscribers() {
        let bus = EventBus::new(16);
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();
        bus.emit(SmtpEvent::TlsUpgraded { id: 1 });
        assert!(matches!(
            rx1.recv().await.unwrap().event,
            SmtpEvent::TlsUpgraded { id: 1 }
        ));
        assert!(matches!(
            rx2.recv().await.unwrap().event,
            SmtpEvent::TlsUpgraded { id: 1 }
        ));
    }

    #[tokio::test]
    async fn broadcast_event_json_cached_once_per_envelope() {
        let bus = EventBus::new(16);
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();
        bus.emit(SmtpEvent::ConnectionClosed { id: 7 });
        let env1 = rx1.recv().await.unwrap();
        let env2 = rx2.recv().await.unwrap();
        // Both receivers share the same envelope Arc — first .json()
        // call serialises, subsequent calls return the cached Arc<str>.
        assert!(Arc::ptr_eq(&env1, &env2));
        let j1 = env1.json();
        let j2 = env2.json();
        assert!(Arc::ptr_eq(&j1, &j2));
        assert!(j1.contains("\"type\":\"ConnectionClosed\""));
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
