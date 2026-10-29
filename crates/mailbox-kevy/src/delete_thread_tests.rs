//! Deleting a conversation: every file of the deleter's copy is handed
//! back, and what other owners still hold stays readable.

use std::sync::Arc;

use kevy_embedded::{Config, Store};

use crate::{KevyMailboxStore, MessageArrival, UserMessageFacts};

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
    let payload = format!(r#"{{"message_id":"{mid}","sender":"other@z.com","thread_id":"{tid}"}}"#);
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

#[test]
fn every_file_a_copy_has_had_is_handed_back() {
    let s = store();
    let u = "u@x.com";
    // filed in Sent first, then the copy that came back to the inbox wrote
    // the same row; and a second inbox copy through an alias
    deliver(&s, u, "t1", "m1", ".Sent/1.M1.host", 1);
    deliver(&s, u, "t1", "m1", "2.M2.host:2,S", 1);
    deliver(&s, u, "t1", "m1", "3.M3.host", 1);
    let (existed, files) = s.delete_thread(u, "t1").unwrap();
    assert!(existed);
    assert_eq!(files, vec![".Sent/1.M1.host", "2.M2.host", "3.M3.host"]);
    let set = crate::keys::user_message_files(u, "m1");
    assert!(
        s.store().smembers(set.as_bytes()).unwrap().is_empty(),
        "the set goes with the copy"
    );
}

#[test]
fn other_owners_keep_what_they_hold() {
    let s = store();
    let (a, b) = ("a@x.com", "b@x.com");
    deliver(&s, a, "t1", "m1", "1.A.host", 7);
    deliver(&s, b, "t1", "m1", "1.B.host", 9);
    deliver(&s, a, "t1", "m2", "2.A.host", 8);

    let (_, files) = s.delete_thread(a, "t1").unwrap();
    assert_eq!(files, vec!["1.A.host", "2.A.host"], "only a's own files");

    // b still opens it, by message and by IMAP uid
    assert!(
        s.get_thread("t1").unwrap().is_some(),
        "the thread record stays"
    );
    assert!(s.get_thread_for_user(b, "t1").unwrap().is_some());
    assert!(s.user_message_view(b, "m1").unwrap().is_some());
    assert!(s.get_message_by_uid(b, 9).unwrap().is_some());
    assert_eq!(s.user_thread_message_ids(b, "t1").unwrap(), vec!["m1"]);
    // and nothing of a's is left
    assert!(s.get_thread_for_user(a, "t1").unwrap().is_none());
    assert!(s.user_message_view(a, "m1").unwrap().is_none());
    assert!(s.get_message_by_uid(a, 7).unwrap().is_none());
    // m2 was a's alone: its shared record goes
    assert!(s.get_message("m2").unwrap().is_none());
    assert!(s.get_message("m1").unwrap().is_some());
    assert_eq!(
        s.thread_message_owners("t1").unwrap(),
        vec![(b.to_string(), "m1".to_string())]
    );
}

#[test]
fn the_last_owner_takes_the_shared_records_with_it() {
    let s = store();
    let (a, b) = ("a@x.com", "b@x.com");
    deliver(&s, a, "t1", "m1", "1.A.host", 1);
    deliver(&s, b, "t1", "m1", "1.B.host", 1);
    s.delete_thread(a, "t1").unwrap();
    let (existed, files) = s.delete_thread(b, "t1").unwrap();
    assert!(existed);
    assert_eq!(files, vec!["1.B.host"]);
    assert!(s.get_thread("t1").unwrap().is_none());
    assert!(s.get_message("m1").unwrap().is_none());
    let zset = crate::keys::thread_messages("t1");
    assert_eq!(s.store().zcard(zset.as_bytes()).unwrap(), 0);
}

#[test]
fn owners_written_before_the_index_was_declared_are_found() {
    let s = KevyMailboxStore::new(Arc::new(Store::open(Config::default()).unwrap()));
    s.ensure_thread_table();
    deliver(&s, "a@x.com", "t1", "m1", "1.A.host", 1);
    deliver(&s, "b@x.com", "t1", "m1", "1.B.host", 1);
    s.ensure_admin_indexes();
    let mut owners = s.thread_message_owners("t1").unwrap();
    owners.sort();
    assert_eq!(
        owners,
        vec![
            ("a@x.com".to_string(), "m1".to_string()),
            ("b@x.com".to_string(), "m1".to_string()),
        ]
    );
    assert!(s.thread_message_owners("t2").unwrap().is_empty());
}

/// Somebody not in a thread deletes nothing, whether the thread has owners
/// or has lost every one of them, and the answer is the one a thread that
/// does not exist gets.
#[test]
fn a_stranger_deletes_nothing() {
    let s = store();
    deliver(&s, "a@x.com", "t-owned", "m1@x", "f1", 1);
    deliver(&s, "b@x.com", "t-orphan", "m2@x", "f2", 1);
    // every owner's row gone, the shared records left behind
    s.store()
        .del(&[crate::keys::thread_user("b@x.com", "t-orphan").as_bytes()])
        .unwrap();
    s.store()
        .del(&[crate::keys::user_message("b@x.com", "m2@x").as_bytes()])
        .unwrap();

    for tid in ["t-owned", "t-orphan", "no-such-thread"] {
        assert_eq!(
            s.delete_thread("stranger@x.com", tid).unwrap(),
            (false, Vec::new()),
            "{tid}"
        );
    }
    assert!(s.get_thread("t-owned").unwrap().is_some());
    assert!(s.get_thread("t-orphan").unwrap().is_some(), "the orphan's shared row went");
    assert!(
        s.store()
            .get(crate::keys::message_blob("m2@x").as_bytes())
            .unwrap()
            .is_some(),
        "the orphan's message went"
    );
    assert!(s.delete_thread("a@x.com", "t-owned").unwrap().0, "the owner still can");
}
