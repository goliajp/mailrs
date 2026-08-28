//! Quarantine is a finding, and nothing else lists it.
//!
//! The same shape as `archive_scope`, and every assertion is about the
//! **pair** — what the page holds and what its total says — because
//! the defect that file records is a filter that fixed the rows and
//! left the count telling the old story. A quarantined conversation
//! that still counts is a reader wondering why the number does not
//! match what they can see.
//!
//! Separate from Junk on purpose. Junk answers "is this worth
//! reading"; this answers "is somebody trying to take something from
//! you", and the mail is kept because it is the evidence the abuse
//! reports are built from.

use crate::KevyMailboxStore;
use crate::list_threads::*;
use crate::thread_row::ThreadRow;
use kevy_embedded::{Config, Store};
use std::sync::Arc;

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
        unread_count: 0,
        latest_date: activity,
        latest_preview: String::new(),
        category: "inbox".into(),
        importance_level: "normal".into(),
        importance_score: 0.0,
        requires_action: false,
        pinned: false,
        archived: false,
        has_action: false,
        sent_count: 0,
        starred: false,
        snoozed_until: 0,
    }
}

fn tids(rows: &[ThreadRow]) -> Vec<&str> {
    rows.iter().map(|r| r.thread_id.as_str()).collect()
}

/// The inbox loses it from the rows **and** from the total.
#[test]
fn quarantining_removes_a_thread_from_the_list_and_the_count() {
    let st = store();
    let u = "alice@x.com";
    for (tid, at) in [("t3", 300), ("t2", 200), ("t1", 100)] {
        st.upsert_thread(u, &row(tid, at)).unwrap();
    }
    let inbox = || {
        st.list_threads_by_activity(u, &ListThreadsFilter::default(), 0, 10)
            .unwrap()
    };

    let (rows, total) = inbox();
    assert_eq!(tids(&rows), ["t3", "t2", "t1"]);
    assert_eq!(total, 3);

    assert!(st.set_quarantined(u, "t2", true).unwrap());

    let (rows, total) = inbox();
    assert_eq!(tids(&rows), ["t3", "t1"], "a held thread was still listed");
    assert_eq!(total, 2, "the count still included a thread nobody can see");
}

/// And releasing puts it back where it was, both ways.
#[test]
fn releasing_returns_it_to_the_list_and_the_count() {
    let st = store();
    let u = "alice@x.com";
    for (tid, at) in [("t2", 200), ("t1", 100)] {
        st.upsert_thread(u, &row(tid, at)).unwrap();
    }
    st.set_quarantined(u, "t1", true).unwrap();
    st.set_quarantined(u, "t1", false).unwrap();

    let (rows, total) = st
        .list_threads_by_activity(u, &ListThreadsFilter::default(), 0, 10)
        .unwrap();
    assert_eq!(tids(&rows), ["t2", "t1"]);
    assert_eq!(total, 2);
}

/// It is one reader's finding. Two people hold the same conversation;
/// holding it for one must not hold it for the other, and releasing it
/// for one must not release it for the other.
#[test]
fn it_is_per_reader_and_not_per_thread() {
    let st = store();
    let (a, b) = ("alice@x.com", "bob@x.com");
    st.upsert_thread(a, &row("t1", 100)).unwrap();
    st.upsert_thread(b, &row("t1", 100)).unwrap();

    st.set_quarantined(a, "t1", true).unwrap();

    let (a_rows, a_total) = st
        .list_threads_by_activity(a, &ListThreadsFilter::default(), 0, 10)
        .unwrap();
    let (b_rows, b_total) = st
        .list_threads_by_activity(b, &ListThreadsFilter::default(), 0, 10)
        .unwrap();
    assert!(
        a_rows.is_empty() && a_total == 0,
        "not held for the reader it was held for"
    );
    assert_eq!(
        tids(&b_rows),
        ["t1"],
        "held for a reader it was not held for"
    );
    assert_eq!(b_total, 1);
}

/// Archiving and quarantining are different states, and a thread can
/// be in one without being in the other. They sit next to each other
/// in the ORDERPATH prefix, which is exactly the arrangement that
/// makes a mistake here silent.
#[test]
fn archiving_and_quarantining_do_not_stand_in_for_each_other() {
    let st = store();
    let u = "alice@x.com";
    for (tid, at) in [("held", 200), ("filed", 100)] {
        st.upsert_thread(u, &row(tid, at)).unwrap();
    }
    st.set_quarantined(u, "held", true).unwrap();
    st.set_archived(u, "filed", true).unwrap();

    let (live, live_total) = st
        .list_threads_by_activity(u, &ListThreadsFilter::default(), 0, 10)
        .unwrap();
    assert!(live.is_empty() && live_total == 0);

    // The archived list is its own axis and must not have picked up
    // the quarantined one.
    let archived = ListThreadsFilter {
        archived: true,
        ..Default::default()
    };
    let (rows, total) = st.list_threads_by_activity(u, &archived, 0, 10).unwrap();
    assert_eq!(
        tids(&rows),
        ["filed"],
        "the archived list showed a held thread"
    );
    assert_eq!(total, 1);
}
