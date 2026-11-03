//! A message whose file is gone leaves the user's mailbox: the row, the
//! conversation's count, and the conversation itself when it was the last.

use std::sync::Arc;

use kevy_embedded::{Config, Store};

use crate::{Forgotten, KevyMailboxStore, MessageArrival, UserMessageFacts};

fn store() -> KevyMailboxStore {
    let s = KevyMailboxStore::new(Arc::new(Store::open(Config::default()).unwrap()));
    s.ensure_thread_table();
    s.ensure_admin_indexes();
    s
}

fn deliver(s: &KevyMailboxStore, user: &str, tid: &str, mid: &str, file: &str, uid: u32) {
    let arrival = MessageArrival {
        thread_id: tid,
        user,
        subject: "Subj",
        senders_csv: "other@z.com",
        latest_date: 100,
        latest_preview: "preview",
        category: "inbox",
        unread: true,
        is_own: false,
    };
    let payload = format!(
        r#"{{"message_id":"{mid}","sender":"other@z.com","thread_id":"{tid}","internal_date":100,"subject":"Subj"}}"#
    );
    let facts = UserMessageFacts {
        blob_ref: file,
        uid,
        flags: 0,
        modseq: 1,
    };
    s.deliver_message(&arrival, mid, payload.as_bytes(), &facts)
        .unwrap();
    s.register_uid(user, uid, mid).unwrap();
}

fn nothing_on_disk(_: &str) -> bool {
    false
}

#[test]
fn a_removed_message_leaves_its_conversation() {
    let s = store();
    let u = "u@x.com";
    deliver(&s, u, "t1", "m1", "1.A.host", 1);
    deliver(&s, u, "t1", "m2", "2.A.host", 2);
    assert_eq!(s.get_thread_for_user(u, "t1").unwrap().unwrap().count, 2);

    let got = s
        .forget_message_file(u, "m1", "1.A.host:2,S", nothing_on_disk)
        .unwrap();
    assert_eq!(
        got,
        Forgotten::Message {
            thread_id: "t1".into()
        }
    );
    assert_eq!(s.user_thread_message_ids(u, "t1").unwrap(), vec!["m2"]);
    let row = s
        .get_thread_for_user(u, "t1")
        .unwrap()
        .expect("still there");
    assert_eq!(row.count, 1);
    assert_eq!(row.unread_count, 1);
    assert!(s.user_message_view(u, "m1").unwrap().is_none());
    assert!(s.get_message_by_uid(u, 1).unwrap().is_none());
    assert!(
        s.get_message("m1").unwrap().is_none(),
        "nobody else holds it"
    );
}

#[test]
fn the_last_message_takes_the_conversation() {
    let s = store();
    let u = "u@x.com";
    deliver(&s, u, "t1", "m1", "1.A.host", 1);
    let got = s
        .forget_message_file(u, "m1", "1.A.host", nothing_on_disk)
        .unwrap();
    assert_eq!(
        got,
        Forgotten::Thread {
            thread_id: "t1".into()
        }
    );
    assert!(s.get_thread_for_user(u, "t1").unwrap().is_none());
}

#[test]
fn another_owner_keeps_the_message() {
    let s = store();
    let (a, b) = ("a@x.com", "b@x.com");
    deliver(&s, a, "t1", "m1", "1.A.host", 1);
    deliver(&s, a, "t1", "m2", "2.A.host", 2);
    deliver(&s, b, "t1", "m1", "1.B.host", 5);
    s.forget_message_file(a, "m1", "1.A.host", nothing_on_disk)
        .unwrap();
    assert!(s.user_message_view(a, "m1").unwrap().is_none());
    assert!(s.user_message_view(b, "m1").unwrap().is_some());
    assert!(s.get_message_by_uid(b, 5).unwrap().is_some());
    assert_eq!(s.user_thread_message_ids(b, "t1").unwrap(), vec!["m1"]);
    assert_eq!(s.get_thread_for_user(b, "t1").unwrap().unwrap().count, 1);
}

#[test]
fn a_copy_still_on_disk_keeps_the_row_and_is_named_by_it() {
    let s = store();
    let u = "u@x.com";
    deliver(&s, u, "t1", "m1", ".Sent/1.S.host", 1);
    deliver(&s, u, "t1", "m1", "2.I.host", 1);
    let got = s
        .forget_message_file(u, "m1", "2.I.host:2,S", |n| n == ".Sent/1.S.host")
        .unwrap();
    assert_eq!(got, Forgotten::OtherCopy);
    let facts = s.user_message_facts(u, "m1").unwrap().expect("kept");
    assert_eq!(facts.blob_ref, ".Sent/1.S.host");
    assert_eq!(s.get_thread_for_user(u, "t1").unwrap().unwrap().count, 1);
}

#[test]
fn a_message_never_indexed_changes_nothing() {
    let s = store();
    assert_eq!(
        s.forget_message_file("u@x.com", "nope", "9.X.host", nothing_on_disk)
            .unwrap(),
        Forgotten::NoCopy
    );
}
