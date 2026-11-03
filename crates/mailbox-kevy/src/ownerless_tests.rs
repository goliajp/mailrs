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

/// Every key the conversation and its message have, so a refused delete can
/// be shown to have touched none of them.
fn shared_keys_left(s: &KevyMailboxStore, tid: &str, mid: &str) -> usize {
    [
        keys::thread(tid),
        keys::thread_messages(tid),
        keys::message_blob(mid),
    ]
    .iter()
    .map(|k| s.store().exists(&[k.as_bytes()]).unwrap())
    .sum()
}

#[test]
fn a_conversation_somebody_joins_after_the_listing_is_not_deleted() {
    let s = store();
    deliver(&s, "a@x.com", "t-orphan", "m2", 200);
    orphan(&s, "a@x.com", "t-orphan", &["m2"]);
    assert_eq!(s.ownerless_threads().unwrap().len(), 1);
    // between the listing and the delete, an account gets the conversation
    s.store()
        .hset(
            keys::thread_user("b@x.com", "t-orphan").as_bytes(),
            &[(b"tid".as_slice(), b"t-orphan".as_slice())],
        )
        .unwrap();
    assert_eq!(
        s.delete_ownerless_thread("t-orphan").unwrap(),
        OwnerlessDelete::Held
    );
    assert_eq!(
        shared_keys_left(&s, "t-orphan", "m2"),
        3,
        "nothing was deleted"
    );
}

/// The atomic check itself, past the scans that would catch these first.
#[test]
fn the_delete_checks_every_account_again_inside_its_atomic_block() {
    let s = store();
    deliver(&s, "a@x.com", "t-orphan", "m2", 200);
    orphan(&s, "a@x.com", "t-orphan", &["m2"]);
    let users = ["a@x.com".to_string(), "b@x.com".to_string()];

    // a message row for one of its messages
    s.store()
        .hset(
            keys::user_message("b@x.com", "m2").as_bytes(),
            &[(b"blob_ref".as_slice(), b"9.M9.host".as_slice())],
        )
        .unwrap();
    assert_eq!(
        s.delete_if_nobodys("t-orphan", &users).unwrap(),
        OwnerlessDelete::Held
    );
    assert_eq!(shared_keys_left(&s, "t-orphan", "m2"), 3);
    s.store()
        .del(&[keys::user_message("b@x.com", "m2").as_bytes()])
        .unwrap();

    // a membership row
    s.store()
        .hset(
            keys::thread_user("a@x.com", "t-orphan").as_bytes(),
            &[(b"tid".as_slice(), b"t-orphan".as_slice())],
        )
        .unwrap();
    assert_eq!(
        s.delete_if_nobodys("t-orphan", &users).unwrap(),
        OwnerlessDelete::Held
    );
    assert_eq!(shared_keys_left(&s, "t-orphan", "m2"), 3);
    s.store()
        .del(&[keys::thread_user("a@x.com", "t-orphan").as_bytes()])
        .unwrap();

    assert!(matches!(
        s.delete_if_nobodys("t-orphan", &users).unwrap(),
        OwnerlessDelete::Deleted(_)
    ));
    assert_eq!(shared_keys_left(&s, "t-orphan", "m2"), 0);
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
