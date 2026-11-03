use std::sync::Arc;

use kevy_embedded::{Config, Store};

use crate::{KevyMailboxStore, MessageArrival, OwnerlessDelete, UserMessageFacts, keys};

fn store() -> KevyMailboxStore {
    let s = KevyMailboxStore::new(Arc::new(Store::open(Config::default()).unwrap()));
    s.ensure_thread_table();
    s.ensure_admin_indexes();
    s
}

fn deliver(s: &KevyMailboxStore, user: &str, tid: &str, mid: &str, at: i64) {
    let arrival = MessageArrival {
        thread_id: tid,
        user,
        subject: "Subj",
        senders_csv: "other@z.com",
        latest_date: at,
        latest_preview: "preview",
        category: "inbox",
        unread: true,
        is_own: false,
    };
    let payload = format!(r#"{{"message_id":"{mid}","sender":"other@z.com","thread_id":"{tid}"}}"#);
    let facts = UserMessageFacts {
        blob_ref: "1.M1.host",
        uid: 1,
        flags: 0,
        modseq: 1,
    };
    s.deliver_message(&arrival, mid, payload.as_bytes(), &facts)
        .unwrap();
}

/// What production has: the shared record outlived every account's rows.
fn orphan(s: &KevyMailboxStore, user: &str, tid: &str, mids: &[&str]) {
    for mid in mids {
        s.store()
            .del(&[keys::user_message(user, mid).as_bytes()])
            .unwrap();
    }
    s.store()
        .del(&[keys::thread_user(user, tid).as_bytes()])
        .unwrap();
}

#[test]
fn only_a_conversation_nobody_holds_is_listed() {
    let s = store();
    deliver(&s, "a@x.com", "t-held", "m1", 100);
    deliver(&s, "a@x.com", "t-orphan", "m2", 200);
    deliver(&s, "a@x.com", "t-orphan", "m3", 300);
    orphan(&s, "a@x.com", "t-orphan", &["m2", "m3"]);
    let got = s.ownerless_threads().unwrap();
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].thread_id, "t-orphan");
    assert_eq!(got[0].messages, 2);
    assert_eq!(got[0].latest_at, 300);
}

#[test]
fn a_conversation_nobody_holds_is_deleted_and_one_held_is_not() {
    let s = store();
    deliver(&s, "a@x.com", "t-held", "m1", 100);
    deliver(&s, "a@x.com", "t-orphan", "m2", 200);
    orphan(&s, "a@x.com", "t-orphan", &["m2"]);

    assert_eq!(
        s.delete_ownerless_thread("t-held").unwrap(),
        OwnerlessDelete::Held
    );
    assert!(
        s.get_thread_for_user("a@x.com", "t-held")
            .unwrap()
            .is_some()
    );

    let OwnerlessDelete::Deleted(gone) = s.delete_ownerless_thread("t-orphan").unwrap() else {
        panic!("not deleted");
    };
    assert_eq!(gone.messages, 1);
    assert!(s.ownerless_threads().unwrap().is_empty());
    for key in [
        keys::thread("t-orphan"),
        keys::thread_messages("t-orphan"),
        keys::message_blob("m2"),
    ] {
        assert_eq!(s.store().exists(&[key.as_bytes()]).unwrap(), 0, "{key}");
    }
    assert_eq!(
        s.delete_ownerless_thread("t-orphan").unwrap(),
        OwnerlessDelete::Missing,
        "a second delete finds nothing"
    );
}

#[test]
fn a_conversation_another_account_still_holds_is_not_ownerless() {
    let s = store();
    deliver(&s, "a@x.com", "t1", "m1", 100);
    deliver(&s, "b@x.com", "t1", "m1", 100);
    orphan(&s, "a@x.com", "t1", &["m1"]);
    assert!(s.ownerless_threads().unwrap().is_empty());
    assert_eq!(
        s.delete_ownerless_thread("t1").unwrap(),
        OwnerlessDelete::Held
    );
}
