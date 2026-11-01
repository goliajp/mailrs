use mailrs_core_api::webhook_events as ev;

use super::*;
use crate::families::webhooks::{NewWebhook, create};

fn conn(name: &str) -> Connection {
    Connection::connect(&format!("mem://webhook-events-{name}")).expect("kevy")
}

fn subscribe(c: &mut Connection, user: &str, kind: &str) -> i64 {
    create(
        c,
        NewWebhook {
            account_address: user.into(),
            url: "https://hooks.example/in".into(),
            event_type: kind.into(),
            filter_sender: None,
            filter_thread_id: None,
        },
    )
    .expect("subscription")
    .id
}

/// Every queued payload, oldest first.
fn queued(c: &mut Connection) -> Vec<serde_json::Value> {
    webhook_outbox::claim_due(c, i64::MAX, 100)
        .expect("claim")
        .into_iter()
        .map(|e| serde_json::from_str(&e.payload).expect("json"))
        .collect()
}

fn received<'a>(user: &'a str, sender: &'a str) -> Event<'a> {
    Event {
        kind: ev::MAIL_RECEIVED,
        user,
        sender: Some(sender),
        thread_id: Some("t1"),
        data: serde_json::json!({ "message_id": "m1@x", "subject": "Hi" }),
    }
}

#[test]
fn the_payload_carries_the_subscribed_name_a_version_and_the_fields() {
    let p = payload(
        ev::MAIL_RECEIVED,
        &received("u@x", "a@b"),
        "2026-11-01T00:00:00+00:00",
    );
    assert_eq!(p["event"], ev::MAIL_RECEIVED);
    assert_eq!(p["version"], 1);
    assert_eq!(p["data"]["user"], "u@x");
    assert_eq!(p["data"]["sender"], "a@b");
    assert_eq!(p["data"]["thread_id"], "t1");
    assert_eq!(p["data"]["subject"], "Hi");
}

/// The old name keeps working, and reads the old name in the payload.
#[test]
fn a_new_message_subscription_still_gets_received_mail() {
    let mut c = conn("alias");
    subscribe(&mut c, "u@x", ev::NEW_MESSAGE);
    assert_eq!(enqueue(&mut c, &received("u@x", "a@b"), 1).unwrap(), 1);
    let held = Event {
        kind: ev::MAIL_HELD,
        ..received("u@x", "a@b")
    };
    assert_eq!(
        enqueue(&mut c, &held, 1).unwrap(),
        0,
        "held mail is not new mail"
    );
    let got = queued(&mut c);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0]["event"], ev::NEW_MESSAGE);
}

/// Only the user's own subscriptions, only of the kind that happened.
#[test]
fn an_event_reaches_its_user_and_its_kind_only() {
    let mut c = conn("scope");
    subscribe(&mut c, "u@x", ev::MAIL_HELD);
    subscribe(&mut c, "other@x", ev::MAIL_RECEIVED);
    assert_eq!(enqueue(&mut c, &received("u@x", "a@b"), 1).unwrap(), 0);
    assert!(queued(&mut c).is_empty());
}

/// A sender filter is about mail from someone: a delivery receipt carries
/// no sender and does not match it.
#[test]
fn a_filter_skips_events_without_its_field() {
    let mut c = conn("filters");
    create(
        &mut c,
        NewWebhook {
            account_address: "u@x".into(),
            url: "https://hooks.example/in".into(),
            event_type: ev::MAIL_DELIVERED.into(),
            filter_sender: Some("boss@corp.example".into()),
            filter_thread_id: None,
        },
    )
    .unwrap();
    let delivered = Event {
        kind: ev::MAIL_DELIVERED,
        user: "u@x",
        sender: None,
        thread_id: None,
        data: serde_json::json!({ "send_id": "s1", "recipient": "boss@corp.example", "code": 250 }),
    };
    assert_eq!(enqueue(&mut c, &delivered, 1).unwrap(), 0);
}

/// Each thread event, with the state it changed to.
#[test]
fn each_thread_event_carries_its_state() {
    for (kind, field) in [
        (ev::THREAD_READ, "read"),
        (ev::THREAD_ARCHIVED, "archived"),
        (ev::THREAD_TRASHED, "trashed"),
        (ev::THREAD_STARRED, "starred"),
        (ev::THREAD_LABELLED, "labelled"),
    ] {
        let mut c = conn(&format!("thread-{field}"));
        subscribe(&mut c, "u@x", kind);
        let e = Event {
            kind,
            user: "u@x",
            sender: None,
            thread_id: Some("t9"),
            data: serde_json::json!({ field: false }),
        };
        assert_eq!(enqueue(&mut c, &e, 1).unwrap(), 1, "{kind}");
        let got = queued(&mut c);
        assert_eq!(got[0]["event"], kind);
        assert_eq!(got[0]["data"][field], false, "{kind}");
        assert_eq!(got[0]["data"]["thread_id"], "t9", "{kind}");
    }
}

#[test]
fn a_remote_reason_is_cut_short() {
    assert_eq!(reason(&"x".repeat(2000)).chars().count(), 500);
    assert_eq!(reason("550 no such user"), "550 no such user");
}
