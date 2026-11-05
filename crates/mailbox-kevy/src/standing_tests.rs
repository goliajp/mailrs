//! Which tab a conversation is in, where it sorts and the time it shows, and
//! whether it asks for action, are each reader's own: read back from the row
//! the list renders, the list pages and the tab counts, after every kind of
//! mutation.
//!
//! Alice and Bob both received Carol's message. Dave's later message, which
//! the classifier filed under Promotions, reached Alice only. Nothing Alice
//! receives or does may move Bob's conversation between tabs, re-sort his
//! list or change the time his row shows.

use crate::display_tests::{CAROLS, DAVES, Mail, store};
use crate::{ArchiveScope, KevyMailboxStore, ListThreadsFilter, MessageArrival, UserMessageFacts};

const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";
/// Bob's other conversation, between t1's two dates.
const OTHER: Mail = Mail {
    mid: "o1",
    from: "erin@outside.example",
    subject: "Lunch",
    body: "Lunch on Thursday?",
    date: 150,
};

fn payload(m: &Mail, tid: &str) -> String {
    format!(
        r#"{{"message_id":"{}","sender":"{}","thread_id":"{tid}","internal_date":{},"subject":"{}","flags":0}}"#,
        m.mid, m.from, m.date, m.subject
    )
}

/// Delivery as fastcore's ingest leaves it, with the category the
/// classifier gave this recipient's copy.
fn deliver(s: &KevyMailboxStore, user: &str, tid: &str, m: &Mail, category: &str) {
    let arrival = MessageArrival {
        thread_id: tid,
        user,
        subject: m.subject,
        senders_csv: m.from,
        latest_date: m.date,
        latest_preview: m.body,
        category,
        unread: true,
        is_own: crate::senders_csv_contains_user(m.from, user),
    };
    let file = format!("{}.{user}", m.mid);
    let facts = UserMessageFacts {
        blob_ref: &file,
        uid: 0,
        flags: 0,
        modseq: 1,
    };
    s.deliver_message(&arrival, m.mid, payload(m, tid).as_bytes(), &facts)
        .unwrap();
}

/// Carol's to both, Bob's other conversation, then Dave's to Alice only.
fn shared(s: &KevyMailboxStore) {
    deliver(s, ALICE, "t1", &CAROLS, "inbox");
    deliver(s, BOB, "t1", &CAROLS, "inbox");
    deliver(s, BOB, "t2", &OTHER, "inbox");
    deliver(s, ALICE, "t1", &DAVES, "promotion");
}

/// Writes that rebuild Bob's row from a ThreadRow: the membership refresh,
/// and his own reply, which the sent route writes from the shared hash.
fn bob_writes(s: &KevyMailboxStore) {
    s.backfill_thread_user(BOB, 0, 10).unwrap();
    crate::display_tests::sent_copy(s, BOB, "t1", "b1", 120);
}

fn ids(s: &KevyMailboxStore, user: &str, folder: &str) -> Vec<String> {
    let f = ListThreadsFilter {
        folder: Some(folder),
        ..Default::default()
    };
    s.list_threads_by_activity(user, &f, 0, 10)
        .unwrap()
        .0
        .into_iter()
        .map(|r| r.thread_id)
        .collect()
}

fn tab_count(s: &KevyMailboxStore, user: &str, bucket: &str) -> usize {
    s.count_thread_ids_by_bucket_via_table(user, bucket, ArchiveScope::Live)
        .unwrap()
}

/// Bob's t1 is in his Inbox at Carol's time, so below his other
/// conversation, and nothing of it counts in his Promotions tab.
fn bob_untouched(s: &KevyMailboxStore, step: &str) {
    let r = s.get_thread_for_user(BOB, "t1").unwrap().unwrap();
    assert_eq!(r.category, "inbox", "{step}: Bob's category");
    assert_eq!(r.latest_date, CAROLS.date, "{step}: Bob's time");
    assert!(!r.requires_action, "{step}: Bob's requires_action");
    assert_eq!(
        ids(s, BOB, "inbox"),
        ["t2", "t1"],
        "{step}: Bob's Inbox order"
    );
    assert!(
        ids(s, BOB, "promotions").is_empty(),
        "{step}: Bob's Promotions"
    );
    assert_eq!(tab_count(s, BOB, "inbox"), 2, "{step}: Bob's Inbox count");
    assert_eq!(
        tab_count(s, BOB, "promotions"),
        0,
        "{step}: Bob's Promotions count"
    );
}

#[test]
fn mail_only_one_member_received_moves_only_their_row() {
    let s = store();
    shared(&s);
    let a = s.get_thread_for_user(ALICE, "t1").unwrap().unwrap();
    assert_eq!(
        (a.category.as_str(), a.latest_date),
        ("promotion", DAVES.date)
    );
    bob_untouched(&s, "arrival");
    s.backfill_thread_user(BOB, 0, 10).unwrap();
    bob_untouched(&s, "membership refresh");
    s.set_thread_display_date(BOB, "t1", CAROLS.date).unwrap();
    bob_untouched(&s, "display date");
}

#[test]
fn a_reply_of_the_readers_own_keeps_their_tab_and_time() {
    let s = store();
    shared(&s);
    bob_writes(&s);
    bob_untouched(&s, "Bob's reply");
}

#[test]
fn one_members_move_is_theirs_alone() {
    let s = store();
    shared(&s);
    assert!(
        s.set_bucket(ALICE, "t1", crate::keys::Bucket::Junk)
            .unwrap()
    );
    assert_eq!(ids(&s, ALICE, "junk"), ["t1"]);
    bob_writes(&s);
    bob_untouched(&s, "Alice's move");
    // and Bob's own move is his
    assert!(
        s.set_bucket(BOB, "t1", crate::keys::Bucket::Promotions)
            .unwrap()
    );
    assert_eq!(ids(&s, BOB, "promotions"), ["t1"]);
    assert_eq!(ids(&s, ALICE, "junk"), ["t1"], "Bob's move left Alice's");
}

#[test]
fn a_readers_move_holds_until_mail_arrives_for_them() {
    let s = store();
    deliver(&s, BOB, "t1", &CAROLS, "inbox");
    s.set_bucket(BOB, "t1", crate::keys::Bucket::Promotions)
        .unwrap();
    s.backfill_thread_user(BOB, 0, 10).unwrap();
    assert_eq!(ids(&s, BOB, "promotions"), ["t1"], "refresh");
    let later = Mail {
        mid: "m9",
        date: 300,
        ..CAROLS
    };
    deliver(&s, BOB, "t1", &later, "inbox");
    assert_eq!(ids(&s, BOB, "inbox"), ["t1"], "the new message's verdict");
}

#[test]
fn one_members_action_flags_are_theirs_alone() {
    let s = store();
    shared(&s);
    // an import of Alice's summary that asks for action, which also puts
    // the flag on the shared hash, and Alice's own marks
    let mut row = s.get_thread_for_user(ALICE, "t1").unwrap().unwrap();
    row.requires_action = true;
    s.upsert_thread(ALICE, &row).unwrap();
    s.set_requires_action(ALICE, "t1", true).unwrap();
    s.set_has_action(ALICE, "t1", true).unwrap();
    assert!(
        s.get_thread_for_user(ALICE, "t1")
            .unwrap()
            .unwrap()
            .requires_action
    );
    bob_writes(&s);
    bob_untouched(&s, "Alice's action");
    assert!(
        !s.get_thread_for_user(BOB, "t1")
            .unwrap()
            .unwrap()
            .has_action
    );
}

#[test]
fn one_members_archive_and_delete_leave_the_others_row() {
    let s = store();
    shared(&s);
    s.set_archived(ALICE, "t1", true).unwrap();
    bob_writes(&s);
    bob_untouched(&s, "Alice's archive");
    s.delete_thread(ALICE, "t1").unwrap();
    s.backfill_thread_user(BOB, 0, 10).unwrap();
    bob_untouched(&s, "Alice's delete");
}

#[test]
fn an_expunge_recounts_from_the_readers_own_messages() {
    let s = store();
    let early = Mail {
        mid: "m0",
        date: 50,
        ..CAROLS
    };
    deliver(&s, BOB, "t1", &early, "promotion");
    shared(&s);
    // Bob's newest goes; his row falls back to his own earlier message,
    // never to Alice's newer one
    s.forget_message_file(BOB, "m1", &format!("m1.{BOB}"), |_| false)
        .unwrap();
    let r = s.get_thread_for_user(BOB, "t1").unwrap().unwrap();
    assert_eq!((r.category.as_str(), r.latest_date), ("promotion", 50));
    // Alice's expunge of Dave's message brings hers back to Carol's
    s.forget_message_file(ALICE, "m2", &format!("m2.{ALICE}"), |_| false)
        .unwrap();
    let a = s.get_thread_for_user(ALICE, "t1").unwrap().unwrap();
    assert_eq!((a.category.as_str(), a.latest_date), ("inbox", CAROLS.date));
}

#[test]
fn a_split_and_a_merge_keep_the_readers_own() {
    let s = store();
    let early = Mail {
        mid: "m0",
        date: 50,
        ..CAROLS
    };
    deliver(&s, BOB, "t1", &early, "notification");
    shared(&s);
    let new = s.split_message_to_new_thread(BOB, "m1").unwrap().unwrap();
    let old = s.get_thread_for_user(BOB, "t1").unwrap().unwrap();
    assert_eq!(
        (old.category.as_str(), old.latest_date),
        ("notification", 50)
    );
    let split = s.get_thread_for_user(BOB, &new).unwrap().unwrap();
    assert_eq!(
        (split.category.as_str(), split.latest_date),
        ("inbox", CAROLS.date)
    );

    s.merge_thread_into(BOB, &new, "t1").unwrap();
    let merged = s.get_thread_for_user(BOB, "t1").unwrap().unwrap();
    assert_eq!(
        (merged.category.as_str(), merged.latest_date),
        ("inbox", CAROLS.date)
    );
    assert_eq!(ids(&s, BOB, "inbox"), ["t2", "t1"]);
}

#[test]
fn a_recount_puts_back_the_readers_own_and_converges() {
    let s = store();
    shared(&s);
    // what the copy from the shared hash used to leave on Bob's row
    s.write_standing(BOB, "t1", Some("promotion"), Some(DAVES.date), None)
        .unwrap();
    assert_eq!(ids(&s, BOB, "inbox"), ["t2"]);
    let dry = s.recount_standing(BOB, "t1", true).unwrap();
    assert_eq!(
        dry,
        crate::StandingRecount {
            held: true,
            activity_from: DAVES.date,
            activity_to: CAROLS.date,
            category_from: "promotion".into(),
            category_to: Some("inbox".into()),
            differs: true,
        }
    );
    assert_eq!(ids(&s, BOB, "inbox"), ["t2"], "a dry run writes nothing");
    // the sort key alone moves the row in the declared order
    s.write_standing(BOB, "t1", Some("inbox"), None, None)
        .unwrap();
    assert_eq!(
        ids(&s, BOB, "inbox"),
        ["t1", "t2"],
        "sorted by the new activity"
    );
    assert!(s.recount_standing(BOB, "t1", false).unwrap().differs);
    bob_untouched(&s, "recount");
    assert!(
        !s.recount_standing(BOB, "t1", true).unwrap().differs,
        "converged"
    );
}

#[test]
fn a_message_with_no_recorded_category_leaves_the_rows() {
    let s = store();
    deliver(&s, BOB, "t1", &CAROLS, "inbox");
    // a message stored before its category was recorded
    s.store()
        .hdel(
            crate::keys::user_message(BOB, CAROLS.mid).as_bytes(),
            &[b"category".as_slice()],
        )
        .unwrap();
    s.write_standing(BOB, "t1", Some("promotion"), None, None)
        .unwrap();
    let r = s.recount_standing(BOB, "t1", false).unwrap();
    assert_eq!(r.category_to, None);
    assert!(!r.differs, "{r:?}");
    assert_eq!(ids(&s, BOB, "promotions"), ["t1"]);
    // the placement record answers for it
    s.set_message_placement(
        BOB,
        CAROLS.mid,
        r#"[{"code":"sorted-by-classifier","params":{"class":"notification"}}]"#,
    )
    .unwrap();
    assert_eq!(
        s.recount_standing(BOB, "t1", true)
            .unwrap()
            .category_to
            .as_deref(),
        Some("notification")
    );
}

#[test]
fn messages_with_no_known_date_leave_the_rows_time() {
    let s = store();
    deliver(&s, BOB, "t1", &CAROLS, "inbox");
    // a message an old backfill indexed at 0
    s.store()
        .zadd(
            crate::keys::thread_user_messages(BOB, "t1").as_bytes(),
            &[(0.0, CAROLS.mid.as_bytes())],
        )
        .unwrap();
    let r = s.recount_standing(BOB, "t1", false).unwrap();
    assert!(r.held && !r.differs, "{r:?}");
    assert_eq!(
        s.get_thread_for_user(BOB, "t1")
            .unwrap()
            .unwrap()
            .latest_date,
        CAROLS.date
    );
}
