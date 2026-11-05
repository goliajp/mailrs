//! A conversation's participants are each reader's own: the senders of the
//! messages that reader holds, read back from the row the list renders.
//!
//! The shared thread hash accumulates every sender any member received, and
//! the membership row used to be copied from it, so a member who joined a
//! conversation later listed people who had never written to them.

use std::sync::Arc;

use kevy_embedded::{Config, Store};

use crate::{KevyMailboxStore, MessageArrival, UserMessageFacts};

const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";
const CAROL: &str = "carol@outside.example";
const DAVE: &str = "dave@outside.example";

fn store() -> KevyMailboxStore {
    let s = KevyMailboxStore::new(Arc::new(Store::open(Config::default()).unwrap()));
    s.ensure_thread_table();
    s.ensure_admin_indexes();
    s
}

fn payload(mid: &str, tid: &str, sender: &str, date: i64) -> String {
    format!(
        r#"{{"message_id":"{mid}","sender":"{sender}","thread_id":"{tid}","internal_date":{date},"subject":"Subj","flags":0}}"#
    )
}

fn facts(file: &str) -> UserMessageFacts<'_> {
    UserMessageFacts {
        blob_ref: file,
        uid: 0,
        flags: 0,
        modseq: 1,
    }
}

/// Inbound delivery, the order `deliver_message` writes it in.
fn deliver(s: &KevyMailboxStore, user: &str, tid: &str, mid: &str, sender: &str, date: i64) {
    let arrival = MessageArrival {
        thread_id: tid,
        user,
        subject: "Subj",
        senders_csv: sender,
        latest_date: date,
        latest_preview: "preview",
        category: "inbox",
        unread: true,
        is_own: crate::senders_csv_contains_user(sender, user),
    };
    let file = format!("{mid}.{user}");
    s.deliver_message(
        &arrival,
        mid,
        payload(mid, tid, sender, date).as_bytes(),
        &facts(&file),
    )
    .unwrap();
}

/// A sent copy, the order fastcore's sent / import route writes it in:
/// the arrival, then the message row, then the row re-emitted from the
/// shared hash.
fn sent_copy(s: &KevyMailboxStore, user: &str, tid: &str, mid: &str, date: i64) {
    s.record_message_arrival(&MessageArrival {
        thread_id: tid,
        user,
        subject: "Re: Subj",
        senders_csv: user,
        latest_date: date,
        latest_preview: "reply",
        category: "inbox",
        unread: false,
        is_own: true,
    })
    .unwrap();
    let file = format!("{mid}.{user}");
    s.upsert_user_message(
        user,
        tid,
        mid,
        date,
        payload(mid, tid, user, date).as_bytes(),
        &facts(&file),
    )
    .unwrap();
    let shared = s.get_thread(tid).unwrap().unwrap();
    s.upsert_thread(user, &shared).unwrap();
}

fn listed(s: &KevyMailboxStore, user: &str, tid: &str) -> String {
    s.get_thread_for_user(user, tid)
        .unwrap()
        .unwrap_or_else(|| panic!("{user} has no row for {tid}"))
        .senders_csv
}

#[test]
fn a_member_who_joins_later_does_not_see_an_earlier_participant() {
    let s = store();
    deliver(&s, ALICE, "t1", "m1", CAROL, 100);
    deliver(&s, ALICE, "t1", "m2", DAVE, 200);
    deliver(&s, BOB, "t1", "m2", DAVE, 200);
    assert_eq!(listed(&s, ALICE, "t1"), format!("{CAROL},{DAVE}"));
    assert_eq!(listed(&s, BOB, "t1"), DAVE, "Carol never wrote to Bob");
}

#[test]
fn a_member_whose_only_message_is_their_own_reply_lists_only_themselves() {
    let s = store();
    deliver(&s, ALICE, "t1", "m1", CAROL, 100);
    sent_copy(&s, BOB, "t1", "m2", 200);
    deliver(&s, ALICE, "t1", "m2", BOB, 200);
    assert_eq!(listed(&s, BOB, "t1"), BOB);
    assert_eq!(listed(&s, ALICE, "t1"), format!("{CAROL},{BOB}"));
}

#[test]
fn an_arrival_for_one_member_does_not_reach_another() {
    let s = store();
    deliver(&s, BOB, "t1", "m1", DAVE, 100);
    deliver(&s, ALICE, "t1", "m1", DAVE, 100);
    deliver(&s, ALICE, "t1", "m2", CAROL, 200);
    // A write that refreshes Bob's row from the shared hash.
    s.set_thread_display_date(BOB, "t1", 150).unwrap();
    s.mark_seen(BOB, "t1").unwrap();
    assert_eq!(listed(&s, BOB, "t1"), DAVE);
    assert_eq!(listed(&s, ALICE, "t1"), format!("{DAVE},{CAROL}"));
}

#[test]
fn an_expunged_message_takes_its_sender_off_the_list() {
    let s = store();
    deliver(&s, ALICE, "t1", "m1", CAROL, 100);
    deliver(&s, ALICE, "t1", "m2", DAVE, 200);
    s.forget_message_file(ALICE, "m1", &format!("m1.{ALICE}"), |_| false)
        .unwrap();
    assert_eq!(listed(&s, ALICE, "t1"), DAVE);
}

#[test]
fn a_split_message_takes_its_sender_to_the_new_conversation() {
    let s = store();
    deliver(&s, ALICE, "t1", "m1", CAROL, 100);
    deliver(&s, ALICE, "t1", "m2", DAVE, 200);
    let new = s.split_message_to_new_thread(ALICE, "m2").unwrap().unwrap();
    assert_eq!(listed(&s, ALICE, "t1"), CAROL);
    assert_eq!(listed(&s, ALICE, &new), DAVE);
}

#[test]
fn a_merge_lists_the_readers_senders_from_both_sides() {
    let s = store();
    deliver(&s, ALICE, "t1", "m1", CAROL, 100);
    deliver(&s, ALICE, "t2", "m2", DAVE, 200);
    // Bob holds only t2, and the merge is Alice's.
    deliver(&s, BOB, "t2", "m2", DAVE, 200);
    s.merge_thread_into(ALICE, "t2", "t1").unwrap();
    assert_eq!(listed(&s, ALICE, "t1"), format!("{CAROL},{DAVE}"));
    assert_eq!(listed(&s, BOB, "t2"), DAVE);
}

#[test]
fn a_folder_move_keeps_the_readers_own_list() {
    let s = store();
    deliver(&s, ALICE, "t1", "m1", CAROL, 100);
    deliver(&s, BOB, "t1", "m2", DAVE, 200);
    deliver(&s, ALICE, "t1", "m2", DAVE, 200);
    s.move_category(BOB, "t1", "promotion").unwrap();
    assert_eq!(listed(&s, BOB, "t1"), DAVE);
}

#[test]
fn a_recount_converges() {
    let s = store();
    deliver(&s, ALICE, "t1", "m1", CAROL, 100);
    deliver(&s, BOB, "t1", "m2", DAVE, 200);
    assert!(!s.recount_participants(ALICE, "t1", false).unwrap());
    assert!(!s.recount_participants(BOB, "t1", false).unwrap());
    assert_eq!(
        s.participants_from_messages(BOB, "t1").unwrap(),
        DAVE.to_string()
    );
}

#[test]
fn a_stored_encoded_sender_is_listed_by_name() {
    // A message stored before intake decoded headers, and its arrival as
    // intake writes it now: a recount must not put the encoded words back.
    let s = store();
    let named = "山田 <yamada@outside.example>";
    let arrival = MessageArrival {
        thread_id: "t1",
        user: ALICE,
        subject: "Subj",
        senders_csv: named,
        latest_date: 100,
        latest_preview: "preview",
        category: "inbox",
        unread: true,
        is_own: false,
    };
    let stored = payload(
        "m1",
        "t1",
        "=?UTF-8?B?5bGx55Sw?= <yamada@outside.example>",
        100,
    );
    let file = format!("m1.{ALICE}");
    s.deliver_message(&arrival, "m1", stored.as_bytes(), &facts(&file))
        .unwrap();
    assert_eq!(s.participants_from_messages(ALICE, "t1").unwrap(), named);
    assert!(!s.recount_participants(ALICE, "t1", true).unwrap());
}
