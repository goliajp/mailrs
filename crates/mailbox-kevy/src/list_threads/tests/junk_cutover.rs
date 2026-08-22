use crate::KevyMailboxStore;
use crate::list_threads::*;
use crate::thread_row::ThreadRow;
use kevy_embedded::{Config, Store};
use std::sync::Arc;

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
        has_action: false,
        sent_count: 0,
        starred: false,
        snoozed_until: 0,
    }
}

fn seeded() -> KevyMailboxStore {
    let st = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    st.ensure_thread_table();
    for (tid, when) in [("j1", 100), ("j2", 200), ("j3", 300), ("j4", 400)] {
        st.upsert_thread("alice@x.com", &row(tid, when, "spam"))
            .unwrap();
    }
    st.upsert_thread("alice@x.com", &row("keep", 500, "inbox"))
        .unwrap();
    st
}

fn junk_filter<'a>() -> ListThreadsFilter<'a> {
    ListThreadsFilter {
        folder: Some("Junk"),
        ..Default::default()
    }
}

/// The served page must be the same threads in the same order the
/// zset path produced, including the total used for paging.
#[test]
fn junk_page_is_newest_first_and_excludes_other_buckets() {
    let st = seeded();
    let (rows, total) = st
        .list_threads_by_activity("alice@x.com", &junk_filter(), 0, 10)
        .unwrap();
    assert_eq!(total, 4);
    let ids: Vec<&str> = rows.iter().map(|r| r.thread_id.as_str()).collect();
    assert_eq!(ids, vec!["j4", "j3", "j2", "j1"]);
}

/// The exclusion the zset encoded by omission: a thread the user
/// only ever sent belongs in Sent, not in their inbox. On prod
/// this was 72 threads for one account.
#[test]
fn inbox_excludes_sent_only_threads() {
    let st = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    st.ensure_thread_table();

    // Received: alice is not among the senders.
    st.upsert_thread("alice@x.com", &row("received", 100, "inbox"))
        .unwrap();
    // Sent-only: alice is the sole sender.
    let mut mine = row("mine", 200, "inbox");
    mine.senders_csv = "alice@x.com".into();
    mine.sent_count = mine.count;
    st.upsert_thread("alice@x.com", &mine).unwrap();

    let filter = ListThreadsFilter {
        folder: Some("Inbox"),
        ..Default::default()
    };
    let (rows, total) = st
        .list_threads_by_activity("alice@x.com", &filter, 0, 10)
        .unwrap();
    let ids: Vec<&str> = rows.iter().map(|r| r.thread_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["received"],
        "a sent-only thread must not reach the inbox"
    );
    assert_eq!(total, 1, "the count must exclude it too, not just the page");
}

/// The case that "has ever sent" got wrong: a conversation the
/// user took part in is still theirs to read. Reading the flag as
/// "is a sender" dropped 190 of one account's inbox threads.
#[test]
fn inbox_keeps_threads_the_user_replied_in() {
    let st = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    st.ensure_thread_table();

    let mut replied = row("replied", 100, "inbox");
    replied.senders_csv = "bob@y.com,alice@x.com".into();
    replied.count = 3;
    replied.sent_count = 1;
    st.upsert_thread("alice@x.com", &replied).unwrap();

    let filter = ListThreadsFilter {
        folder: Some("Inbox"),
        ..Default::default()
    };
    let (rows, total) = st
        .list_threads_by_activity("alice@x.com", &filter, 0, 10)
        .unwrap();
    assert_eq!(
        rows.iter()
            .map(|r| r.thread_id.as_str())
            .collect::<Vec<_>>(),
        vec!["replied"],
        "a thread the user replied in must stay in the inbox"
    );
    assert_eq!(total, 1);
}

/// Offset paging must not repeat or skip across page boundaries.
#[test]
fn junk_offset_paging_is_contiguous() {
    let st = seeded();
    let (p1, _) = st
        .list_threads_by_activity("alice@x.com", &junk_filter(), 0, 2)
        .unwrap();
    let (p2, _) = st
        .list_threads_by_activity("alice@x.com", &junk_filter(), 2, 2)
        .unwrap();
    let ids: Vec<&str> = p1
        .iter()
        .chain(p2.iter())
        .map(|r| r.thread_id.as_str())
        .collect();
    assert_eq!(ids, vec!["j4", "j3", "j2", "j1"]);
}

/// "Load more" passes the tail's timestamp; the next page must be
/// strictly older, which is the range the composite answers.
#[test]
fn junk_cursor_returns_strictly_older_threads() {
    let st = seeded();
    let filter = ListThreadsFilter {
        before_ts: Some(300),
        ..junk_filter()
    };
    let (rows, _) = st
        .list_threads_by_activity("alice@x.com", &filter, 0, 10)
        .unwrap();
    let ids: Vec<&str> = rows.iter().map(|r| r.thread_id.as_str()).collect();
    assert_eq!(ids, vec!["j2", "j1"]);
}
