//! A hold leaves what the reader has read as it was, and the badge and
//! "mark all as read" leave held conversations out.
//!
//! Holding used to mark the conversation read. It was released read, so
//! the reader never noticed it was back, and the badge, which counted held
//! conversations, looked right only because they were all read: a message
//! arriving in a held conversation made the badge count mail the reader
//! could not find.

use axum::Json;
use axum::extract::{Path, State};

use crate::ingest::ingest_delivered_file;
use crate::tests::fresh_state;

const USER: &str = "bob@golia.jp";
const TID: &str = "p1@outside.example";

const PHISH: &[u8] = b"From: Bank <alerts@outside.example>\r\n\
To: bob@golia.jp\r\n\
Subject: Your account is locked\r\n\
Message-ID: <p1@outside.example>\r\n\
Date: Tue, 5 Aug 2026 09:00:00 +0900\r\n\
\r\n\
Confirm your password here.\r\n";

const FOLLOW_UP: &[u8] = b"From: Bank <alerts@outside.example>\r\n\
To: bob@golia.jp\r\n\
Subject: Re: Your account is locked\r\n\
Message-ID: <p2@outside.example>\r\n\
In-Reply-To: <p1@outside.example>\r\n\
References: <p1@outside.example>\r\n\
Date: Tue, 5 Aug 2026 10:00:00 +0900\r\n\
\r\n\
Last reminder.\r\n";

const ORDINARY: &[u8] = b"From: Carol <carol@example.com>\r\n\
To: bob@golia.jp\r\n\
Subject: Quarterly report\r\n\
Message-ID: <q1@example.com>\r\n\
Date: Tue, 5 Aug 2026 11:00:00 +0900\r\n\
\r\n\
Please review the figures.\r\n";

pub(super) fn held_verdict() -> String {
    serde_json::json!({
        "rules_version": "2026-11-05.1",
        "score": 9.5,
        "quarantined": true,
        "layers": [],
    })
    .to_string()
}

fn unread_count(state: &crate::FastcoreState, tid: &str) -> i64 {
    state
        .mailbox
        .get_thread_for_user(USER, tid)
        .expect("read")
        .expect("row")
        .unread_count
}

fn quarantined(state: &crate::FastcoreState, tid: &str) -> bool {
    state
        .mailbox
        .get_thread_for_user(USER, tid)
        .expect("read")
        .expect("row")
        .quarantined
}

fn message_unread(state: &crate::FastcoreState, mid: &str) -> bool {
    let facts = state
        .mailbox
        .user_message_facts(USER, mid)
        .expect("read")
        .expect("row");
    facts.flags & mailrs_core_api::method::message::FLAG_SEEN == 0
}

async fn badge(state: &std::sync::Arc<crate::FastcoreState>) -> (i64, i64) {
    let Json(r) = crate::routes::get_unseen_count(State(state.clone()), Path(USER.into())).await;
    (r.count, r.inbox)
}

async fn release(state: &std::sync::Arc<crate::FastcoreState>, tid: &str) {
    let res =
        crate::routes::release_quarantined(State(state.clone()), Path((USER.into(), tid.into())))
            .await;
    assert!(
        res.status().is_success(),
        "release answered {}",
        res.status()
    );
}

fn arrive_held(state: &std::sync::Arc<crate::FastcoreState>) {
    let v = held_verdict();
    ingest_delivered_file(state, USER, "p1.eml", PHISH, "INBOX", Some(&v));
    assert!(quarantined(state, TID), "the verdict did not hold it");
}

#[test]
fn a_hold_on_arrival_leaves_the_conversation_unread() {
    let state = fresh_state();
    arrive_held(&state);
    assert_eq!(unread_count(&state, TID), 1, "held and marked read");
    assert!(message_unread(&state, TID), "the message row says read");
}

#[tokio::test]
async fn the_badge_leaves_a_held_conversation_out() {
    let state = fresh_state();
    arrive_held(&state);
    ingest_delivered_file(&state, USER, "q1.eml", ORDINARY, "INBOX", None);
    assert_eq!(badge(&state).await, (1, 1), "the badge counted held mail");
}

/// The latent half: a message with no verdict arriving in a held
/// conversation is unread, and the badge counted it while no list showed
/// it, before holds stopped marking anything read.
#[tokio::test]
async fn mail_arriving_in_a_held_conversation_is_unread_and_out_of_the_badge() {
    let state = fresh_state();
    arrive_held(&state);
    ingest_delivered_file(&state, USER, "p2.eml", FOLLOW_UP, "INBOX", None);
    assert!(quarantined(&state, TID), "the follow-up released it");
    assert!(message_unread(&state, "p2@outside.example"));
    assert_eq!(badge(&state).await, (0, 0), "the badge counted held mail");

    release(&state, TID).await;
    assert_eq!(unread_count(&state, TID), 2, "released, but not unread");
    assert_eq!(
        badge(&state).await,
        (1, 1),
        "released, and not in the badge"
    );
}

#[tokio::test]
async fn a_release_returns_the_conversation_unread_and_into_the_badge() {
    let state = fresh_state();
    arrive_held(&state);
    release(&state, TID).await;
    assert!(!quarantined(&state, TID));
    assert_eq!(unread_count(&state, TID), 1, "released read");
    assert!(message_unread(&state, TID));
    assert_eq!(badge(&state).await, (1, 1));
}

#[tokio::test]
async fn mark_all_read_leaves_held_conversations_alone() {
    let state = fresh_state();
    arrive_held(&state);
    // unread on the old code too, which marked the hold read
    ingest_delivered_file(&state, USER, "p2.eml", FOLLOW_UP, "INBOX", None);
    ingest_delivered_file(&state, USER, "q1.eml", ORDINARY, "INBOX", None);

    let Json(r) = crate::routes::mark_all_read_route(State(state.clone()), Path(USER.into())).await;
    assert_eq!(r["flipped"], 1, "{r}");
    assert_eq!(unread_count(&state, "q1@example.com"), 0);
    assert_eq!(
        unread_count(&state, TID),
        2,
        "mark all read touched held mail"
    );

    release(&state, TID).await;
    assert_eq!(unread_count(&state, TID), 2);
}

/// A rescan or a re-judge holding a conversation leaves it as read as it
/// was: unread stays unread, read stays read.
#[test]
fn a_rescan_hold_leaves_what_was_read_as_it_was() {
    let state = fresh_state();
    ingest_delivered_file(&state, USER, "p1.eml", PHISH, "INBOX", None);
    ingest_delivered_file(&state, USER, "q1.eml", ORDINARY, "INBOX", None);
    crate::routes::thread_actions::mark_thread_read_everywhere(&state, USER, "q1@example.com");
    let v: mailrs_inbound::FraudVerdict = serde_json::from_str(&held_verdict()).expect("verdict");

    for (tid, source) in [(TID, "rescan"), ("q1@example.com", "recall")] {
        let held =
            crate::maintenance::fraud_rescan::holding::apply(&state, USER, tid, tid, &v, source)
                .expect("apply");
        assert!(held, "{tid} was not held");
    }
    assert_eq!(unread_count(&state, TID), 1, "the rescan marked it read");
    assert!(message_unread(&state, TID));
    assert_eq!(
        unread_count(&state, "q1@example.com"),
        0,
        "the rescan marked it unread"
    );
}
