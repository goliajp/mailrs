use mailrs_mailbox::labels::LabelError;

use crate::{KevyMailboxStore, LabelOpError, MessageArrival};

fn store() -> KevyMailboxStore {
    let s = KevyMailboxStore::new(std::sync::Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    s.ensure_thread_table();
    s.ensure_admin_indexes();
    s
}

fn arrive(s: &KevyMailboxStore, user: &str, tid: &str) {
    s.record_message_arrival(&MessageArrival {
        thread_id: tid,
        user,
        subject: "s",
        senders_csv: "other@x.com",
        latest_date: 100,
        latest_preview: "",
        category: "inbox",
        unread: true,
        is_own: false,
    })
    .unwrap();
}

const U: &str = "u@x.com";

fn refused(r: Result<impl std::fmt::Debug, LabelOpError>) -> LabelError {
    match r {
        Err(LabelOpError::Label(e)) => e,
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn labels_are_made_renamed_and_deleted() {
    let s = store();
    let work = s.create_label(U, " Work ", "blue", 1).unwrap();
    assert_eq!((work.id.as_str(), work.name.as_str()), ("1", "Work"));
    let home = s.create_label(U, "Home", "green", 2).unwrap();
    assert_eq!(home.id, "2");
    assert_eq!(
        refused(s.create_label(U, "work", "red", 3)),
        LabelError::Taken("work".into())
    );
    assert_eq!(
        refused(s.update_label(U, "2", Some("WORK"), None)),
        LabelError::Taken("WORK".into())
    );
    let renamed = s.update_label(U, "1", Some("Jobs"), Some("red")).unwrap();
    assert_eq!((renamed.id.as_str(), renamed.color.as_str()), ("1", "red"));
    let names: Vec<_> = s.labels(U).unwrap().into_iter().map(|l| l.name).collect();
    assert_eq!(names, ["Home", "Jobs"]);
    assert!(s.delete_label(U, "1").unwrap());
    assert!(!s.delete_label(U, "1").unwrap());
    assert_eq!(s.labels(U).unwrap().len(), 1);
    assert!(
        s.labels("other@x.com").unwrap().is_empty(),
        "labels are per user"
    );
}

#[test]
fn a_label_goes_on_and_off_a_conversation_its_user_is_in() {
    let s = store();
    arrive(&s, U, "t1");
    arrive(&s, "other@x.com", "t-theirs");
    let l = s.create_label(U, "Work", "blue", 1).unwrap();
    s.set_label(U, &l.id, "t1", true).unwrap();
    assert_eq!(s.threads_with_label(U, &l.id).unwrap(), ["t1"]);
    assert_eq!(
        s.labels_by_thread(U).unwrap()["t1"],
        std::slice::from_ref(&l.id)
    );
    assert_eq!(
        refused(s.set_label(U, &l.id, "t-theirs", true)),
        LabelError::NotFound
    );
    assert_eq!(
        refused(s.set_label(U, "99", "t1", true)),
        LabelError::NotFound
    );
    s.set_label(U, &l.id, "t1", false).unwrap();
    assert!(s.threads_with_label(U, &l.id).unwrap().is_empty());
}

#[test]
fn deleting_a_conversation_or_its_label_leaves_nothing_behind() {
    let s = store();
    arrive(&s, U, "t1");
    arrive(&s, U, "t2");
    let a = s.create_label(U, "A", "blue", 1).unwrap();
    let b = s.create_label(U, "B", "red", 1).unwrap();
    for (l, t) in [(&a, "t1"), (&b, "t1"), (&a, "t2")] {
        s.set_label(U, &l.id, t, true).unwrap();
    }
    s.delete_thread(U, "t1").unwrap();
    assert_eq!(s.threads_with_label(U, &a.id).unwrap(), ["t2"]);
    assert!(s.threads_with_label(U, &b.id).unwrap().is_empty());
    s.delete_label(U, &a.id).unwrap();
    assert!(s.labels_by_thread(U).unwrap().is_empty());
}

#[test]
fn a_merged_conversation_keeps_both_halves_labels() {
    let s = store();
    arrive(&s, U, "t-from");
    arrive(&s, U, "t-into");
    let l = s.create_label(U, "A", "blue", 1).unwrap();
    s.set_label(U, &l.id, "t-from", true).unwrap();
    s.merge_thread_into(U, "t-from", "t-into").unwrap();
    assert_eq!(s.threads_with_label(U, &l.id).unwrap(), ["t-into"]);
}
