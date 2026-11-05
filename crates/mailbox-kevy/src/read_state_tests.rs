//! One member reading a conversation leaves every other member's copy as
//! it was: their list, their badge, their IMAP flags, and what a split or
//! merge of their own copy counts as read.
//!
//! `mark_seen` also sets the bit on the shared blob of every message in the
//! conversation, which several readers can hold; these read back what each
//! surface serves the other member afterwards.

use std::sync::Arc;

use kevy_embedded::{Config, Store};

use crate::{KevyMailboxStore, MessageArrival, UserMessageFacts};

const ALICE: &str = "alice@example.test";
pub(crate) const BOB: &str = "bob@example.test";
const CAROL: &str = "carol@outside.example";

fn store() -> KevyMailboxStore {
    let s = KevyMailboxStore::new(Arc::new(Store::open(Config::default()).unwrap()));
    s.ensure_thread_table();
    s.ensure_admin_indexes();
    s
}

fn deliver(s: &KevyMailboxStore, user: &str, tid: &str, mid: &str, date: i64) {
    let arrival = MessageArrival {
        thread_id: tid,
        user,
        subject: "Subj",
        senders_csv: CAROL,
        latest_date: date,
        latest_preview: "preview",
        category: "inbox",
        unread: true,
        is_own: false,
    };
    let payload = format!(
        r#"{{"message_id":"{mid}","sender":"{CAROL}","thread_id":"{tid}","internal_date":{date},"subject":"Subj","flags":0}}"#
    );
    let file = format!("{mid}.{user}");
    let facts = UserMessageFacts {
        blob_ref: &file,
        uid: 1,
        flags: 0,
        modseq: 1,
    };
    s.deliver_message(&arrival, mid, payload.as_bytes(), &facts)
        .unwrap();
}

/// What fastcore's `mark_thread_read_everywhere` does to the store.
fn read(s: &KevyMailboxStore, user: &str, tid: &str) {
    assert!(s.mark_seen(user, tid).unwrap());
    s.mark_thread_messages_seen(user, tid).unwrap();
}

fn unread_listed(s: &KevyMailboxStore, user: &str, tid: &str) -> i64 {
    s.get_thread_for_user(user, tid)
        .unwrap()
        .unwrap()
        .unread_count
}

fn badge(s: &KevyMailboxStore, user: &str) -> usize {
    s.count_flag_in_buckets(user, "unread", &["inbox"]).unwrap()
}

fn imap_seen(s: &KevyMailboxStore, user: &str, mid: &str) -> bool {
    let row = s.user_message_facts(user, mid).unwrap().unwrap();
    let view: serde_json::Value =
        serde_json::from_slice(&s.user_message_view(user, mid).unwrap().unwrap()).unwrap();
    assert_eq!(view["flags"].as_u64().unwrap(), u64::from(row.flags));
    row.flags & 1 != 0
}

fn two_members_one_unread_message() -> KevyMailboxStore {
    let s = store();
    deliver(&s, ALICE, "t1", "m1", 100);
    deliver(&s, BOB, "t1", "m1", 100);
    read(&s, ALICE, "t1");
    s
}

#[test]
fn reading_leaves_the_other_members_list_badge_and_flags_unread() {
    let s = two_members_one_unread_message();
    assert_eq!(unread_listed(&s, ALICE, "t1"), 0);
    assert_eq!(unread_listed(&s, BOB, "t1"), 1, "Bob's list row");
    assert_eq!(badge(&s, BOB), 1, "Bob's badge");
    assert!(!imap_seen(&s, BOB, "m1"), "Bob's IMAP \\Seen");
}

/// Bob splits his copy of the message Alice has read; returns the store
/// and Bob's new conversation.
pub(crate) fn split_after_another_read() -> (KevyMailboxStore, String) {
    let s = two_members_one_unread_message();
    deliver(&s, BOB, "t1", "m0", 50);
    let new = s.split_message_to_new_thread(BOB, "m1").unwrap().unwrap();
    (s, new)
}

#[test]
fn the_other_member_splitting_their_copy_still_has_it_unread() {
    let (s, new) = split_after_another_read();
    assert!(!imap_seen(&s, BOB, "m1"));
    assert_eq!(unread_listed(&s, BOB, &new), 1, "the split row");
    assert_eq!(badge(&s, BOB), 2, "both of Bob's conversations");
}

#[test]
fn the_other_member_merging_their_copy_still_has_it_unread() {
    let s = two_members_one_unread_message();
    deliver(&s, BOB, "t2", "m2", 200);
    s.merge_thread_into(BOB, "t1", "t2").unwrap();
    assert!(!imap_seen(&s, BOB, "m1"));
    assert_eq!(unread_listed(&s, BOB, "t2"), 2);
}
