//! Tests for `event_bus` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

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
    assert!(matches!(rx1.recv().await.unwrap(), SmtpEvent::TlsUpgraded { id: 1 }));
    assert!(matches!(rx2.recv().await.unwrap(), SmtpEvent::TlsUpgraded { id: 1 }));
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
        SmtpEvent::ConnectionOpened { id: 0, addr: "1.2.3.4".into(), tls: false },
        SmtpEvent::CommandReceived { id: 0, command: "EHLO".into(), state_before: "Connected".into() },
        SmtpEvent::ResponseSent { id: 0, response: "250 OK".into(), state_after: "Greeted".into() },
        SmtpEvent::TlsUpgraded { id: 0 },
        SmtpEvent::Authenticated { id: 0, username: "user".into() },
        SmtpEvent::MessageDelivered { id: 0, from: "a@b".into(), to: vec![], size: 0 },
        SmtpEvent::SpamRejected { id: 0, reason: "spam".into() },
        SmtpEvent::MessageQueued { id: 0, from: "a@b".into(), to: vec![] },
        SmtpEvent::ConnectionClosed { id: 0 },
        SmtpEvent::DeliveryAttempt { queue_id: 0, domain: "d".into() },
        SmtpEvent::DeliverySuccess { queue_id: 0, domain: "d".into() },
        SmtpEvent::DeliveryFailed { queue_id: 0, domain: "d".into(), error: "e".into() },
        SmtpEvent::BounceGenerated { queue_id: 0, sender: "a@b".into() },
        SmtpEvent::NewMessage { user: "u".into(), thread_id: "t".into(), sender: "s".into(), subject: "sub".into(), snippet: "sn".into() },
    ];
    for event in events {
        let json = serde_json::to_string(&event);
        assert!(json.is_ok(), "failed to serialize: {event:?}");
    }
}
