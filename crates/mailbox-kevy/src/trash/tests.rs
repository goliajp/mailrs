//! A thread in the Trash is in no other list — Inbox, starred, unread,
//! Archived, the badge — and is in the Trash list, rows and totals alike.
//! Restoring puts it back where it was; the purge finds it only when due.

use std::sync::Arc;

use kevy_embedded::{Config, Store};

use crate::KevyMailboxStore;
use crate::list_threads::*;
use crate::thread_row::ThreadRow;

fn store() -> KevyMailboxStore {
    let s = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    s.ensure_thread_table();
    s
}

fn row(tid: &str, activity: i64) -> ThreadRow {
    ThreadRow {
        account_id: String::new(),
        thread_id: tid.into(),
        subject: "s".into(),
        senders_csv: "a@x.com".into(),
        count: 1,
        unread_count: 1,
        latest_date: activity,
        latest_preview: String::new(),
        category: "inbox".into(),
        importance_level: "normal".into(),
        importance_score: 0.0,
        requires_action: false,
        pinned: false,
        archived: false,
        quarantined: false,
        has_action: false,
        sent_count: 0,
        starred: false,
        snoozed_until: 0,
    }
}

const U: &str = "alice@x.com";

fn list(st: &KevyMailboxStore, f: ListThreadsFilter<'_>) -> (Vec<String>, usize) {
    let (rows, total) = st.list_threads_by_activity(U, &f, 0, 50).unwrap();
    (rows.into_iter().map(|r| r.thread_id).collect(), total)
}

fn inbox<'a>() -> ListThreadsFilter<'a> {
    ListThreadsFilter {
        folder: Some("Inbox"),
        ..Default::default()
    }
}

#[test]
fn the_trash_is_out_of_every_other_list_and_restoring_puts_it_back() {
    let st = store();
    for (tid, at) in [("t3", 300), ("t2", 200), ("t1", 100)] {
        st.upsert_thread(U, &row(tid, at)).unwrap();
    }
    st.set_starred(U, "t2", true).unwrap();
    st.set_archived(U, "t1", true).unwrap();
    for tid in ["t1", "t2", "t3"] {
        st.mark_unread(U, tid).unwrap();
    }
    assert_eq!(list(&st, inbox()).1, 2);
    assert_eq!(
        st.count_flag_non_junk(U, "unread").unwrap(),
        2,
        "badge before"
    );

    assert!(st.set_trashed(U, "t2", true, 1_000).unwrap());
    assert!(st.set_trashed(U, "t1", true, 1_000).unwrap());
    let trash = ListThreadsFilter {
        trashed: true,
        ..Default::default()
    };
    let starred = ListThreadsFilter {
        starred: true,
        ..Default::default()
    };
    let archived = ListThreadsFilter {
        archived: true,
        ..Default::default()
    };
    assert_eq!(list(&st, inbox()), (vec!["t3".to_string()], 1));
    assert_eq!(list(&st, ListThreadsFilter::default()).1, 1);
    assert_eq!(list(&st, starred.clone()).1, 0, "starred lists the trash");
    assert_eq!(list(&st, archived.clone()).1, 0, "Archived lists the trash");
    assert_eq!(
        st.count_flag_non_junk(U, "unread").unwrap(),
        1,
        "the badge counts the trash"
    );
    // the Trash shows both, the archived one too
    assert_eq!(
        list(&st, trash.clone()),
        (vec!["t2".into(), "t1".into()], 2)
    );

    st.set_trashed(U, "t2", false, 2_000).unwrap();
    st.set_trashed(U, "t1", false, 2_000).unwrap();
    assert_eq!(list(&st, inbox()).1, 2);
    assert_eq!(list(&st, starred).0, vec!["t2".to_string()]);
    assert_eq!(list(&st, archived).0, vec!["t1".to_string()]);
    assert_eq!(list(&st, trash).1, 0);
}

#[test]
fn the_purge_finds_only_what_is_due() {
    let st = store();
    for tid in ["old", "new", "kept"] {
        st.upsert_thread(U, &row(tid, 100)).unwrap();
    }
    st.set_trashed(U, "old", true, 1_000).unwrap();
    st.set_trashed(U, "new", true, 5_000).unwrap();
    assert_eq!(
        st.trashed_before(1_000, 10).unwrap(),
        vec![(U.to_string(), "old".to_string())]
    );
    assert!(st.trashed_before(999, 10).unwrap().is_empty());
    st.set_trashed(U, "old", false, 6_000).unwrap();
    assert!(
        st.trashed_before(10_000, 10).unwrap().len() == 1,
        "restored is not due"
    );
    assert_eq!(st.trashed_threads(U).unwrap(), vec!["new".to_string()]);
}

#[test]
fn a_thread_nobody_has_is_not_trashed() {
    let st = store();
    assert!(!st.set_trashed(U, "nope", true, 1).unwrap());
}
