//! All Mail: every bucket but Junk, archived threads included, the
//! Trash left out — and a total that counts what the pages list. The
//! sent-only side is in `message_arrival`, where that fact is derived.

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

fn row(tid: &str, activity: i64, category: &str) -> ThreadRow {
    ThreadRow {
        account_id: String::new(),
        thread_id: tid.into(),
        subject: "s".into(),
        senders_csv: "a@x.com".into(),
        count: 1,
        unread_count: 0,
        latest_date: activity,
        latest_preview: String::new(),
        category: category.into(),
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
        trashed_at: 0,
    }
}

fn tids(rows: &[ThreadRow]) -> Vec<&str> {
    rows.iter().map(|r| r.thread_id.as_str()).collect()
}

fn all_mail() -> ListThreadsFilter<'static> {
    ListThreadsFilter {
        folder: Some("all"),
        ..Default::default()
    }
}

/// Seven threads, one per way a thread can be filed; All Mail lists the
/// four that are mail the user has and has not thrown away.
fn seeded() -> (KevyMailboxStore, &'static str) {
    let st = store();
    let u = "alice@x.com";
    for (tid, at, cat) in [
        ("live", 700, "inbox"),
        ("junk", 600, "spam"),
        ("filed", 500, "inbox"),
        ("note", 400, "notification"),
        ("binned", 300, "inbox"),
        ("older", 200, "inbox"),
        ("filed-promo", 100, "promotion"),
    ] {
        st.upsert_thread(u, &row(tid, at, cat)).unwrap();
    }
    st.set_archived(u, "filed", true).unwrap();
    st.set_archived(u, "filed-promo", true).unwrap();
    st.set_trashed(u, "binned", true, 1).unwrap();
    (st, u)
}

#[test]
fn all_mail_lists_archived_but_not_junk_or_trash() {
    let (st, u) = seeded();
    let (rows, total) = st.list_threads_by_activity(u, &all_mail(), 0, 50).unwrap();
    assert_eq!(
        tids(&rows),
        vec!["live", "filed", "note", "older", "filed-promo"]
    );
    assert_eq!(total, 5, "the total counts what the pages list");
}

#[test]
fn all_mail_pages_by_offset_and_by_cursor_without_gaps() {
    let (st, u) = seeded();
    let (first, _) = st.list_threads_by_activity(u, &all_mail(), 0, 2).unwrap();
    let (second, _) = st.list_threads_by_activity(u, &all_mail(), 2, 2).unwrap();
    assert_eq!(tids(&first), vec!["live", "filed"]);
    assert_eq!(tids(&second), vec!["note", "older"]);

    let after = ListThreadsFilter {
        before_ts: Some(first.last().unwrap().latest_date),
        ..all_mail()
    };
    let (rest, _) = st.list_threads_by_activity(u, &after, 0, 10).unwrap();
    assert_eq!(tids(&rest), vec!["note", "older", "filed-promo"]);
}

/// A flag on top keys on the flag index; both sides of the archive still
/// answer, and Junk still does not.
#[test]
fn all_mail_with_a_flag_reads_both_sides() {
    let (st, u) = seeded();
    for tid in ["live", "filed", "junk", "older"] {
        st.set_starred(u, tid, true).unwrap();
    }
    let f = ListThreadsFilter {
        starred: true,
        ..all_mail()
    };
    let (rows, total) = st.list_threads_by_activity(u, &f, 0, 50).unwrap();
    assert_eq!(tids(&rows), vec!["live", "filed", "older"]);
    assert_eq!(total, 3);
}

/// The Archived and Trash lists keep their own meaning when the folder
/// the client was standing in is All Mail.
#[test]
fn archived_and_trash_are_not_widened_by_all_mail() {
    let (st, u) = seeded();
    let archived = ListThreadsFilter {
        archived: true,
        ..all_mail()
    };
    let (rows, _) = st.list_threads_by_activity(u, &archived, 0, 50).unwrap();
    assert_eq!(tids(&rows), vec!["filed", "filed-promo"]);
}
