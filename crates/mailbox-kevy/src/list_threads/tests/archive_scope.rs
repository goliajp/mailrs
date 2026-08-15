//! Archived is a list of its own, so nothing else lists it.
//!
//! Until 2026-08-05 `archived: false` applied no predicate at all: the
//! Inbox page came back with archived threads in it and the web client
//! deleted them afterwards, from a page whose size the server had
//! already reported. Every assertion here is about the pair — what the
//! page holds *and* what its total says — because a filter that only
//! fixes the rows leaves the count telling the old story.

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

fn tids(rows: &[ThreadRow]) -> Vec<&str> {
    rows.iter().map(|r| r.thread_id.as_str()).collect()
}

/// The Inbox loses exactly what Archived gains — rows and total alike.
#[test]
fn archiving_moves_a_thread_between_the_two_lists() {
    let st = store();
    let u = "alice@x.com";
    for (tid, at) in [("t3", 300), ("t2", 200), ("t1", 100)] {
        st.upsert_thread(u, &row(tid, at, "inbox")).unwrap();
    }

    let inbox = || {
        let f = ListThreadsFilter {
            folder: Some("Inbox"),
            ..Default::default()
        };
        st.list_threads_by_activity(u, &f, 0, 50).unwrap()
    };
    let archived = || {
        let f = ListThreadsFilter {
            archived: true,
            ..Default::default()
        };
        st.list_threads_by_activity(u, &f, 0, 50).unwrap()
    };

    let (_, before) = inbox();
    assert_eq!(before, 3);

    st.set_archived(u, "t2", true).unwrap();

    let (rows, inbox_total) = inbox();
    assert_eq!(tids(&rows), vec!["t3", "t1"], "Inbox must not list it");
    assert_eq!(
        inbox_total, 2,
        "and must not count it — a page the client has to prune is a page \
         whose size it was told wrong"
    );

    let (rows, archived_total) = archived();
    assert_eq!(tids(&rows), vec!["t2"], "Archived must list it");
    assert_eq!(archived_total, 1);
    assert_eq!(
        inbox_total + archived_total,
        before,
        "the two lists partition what was there"
    );
}

/// Un-archiving puts it back, which is the same property read the other
/// way round — and the one that catches an exclusion written into the
/// read but not into the index the write maintains.
#[test]
fn unarchiving_returns_it_to_its_folder() {
    let st = store();
    let u = "alice@x.com";
    st.upsert_thread(u, &row("t", 100, "inbox")).unwrap();
    st.set_archived(u, "t", true).unwrap();
    st.set_archived(u, "t", false).unwrap();

    let f = ListThreadsFilter {
        folder: Some("Inbox"),
        ..Default::default()
    };
    let (rows, total) = st.list_threads_by_activity(u, &f, 0, 50).unwrap();
    assert_eq!(tids(&rows), vec!["t"]);
    assert_eq!(total, 1);
}

/// The axes with no folder and no flag — the default list and the
/// category list — exclude it too. Each reads a different ORDERPATH, and
/// an exclusion added to one prefix and not the others is exactly the
/// shape of bug this is here to catch.
#[test]
fn the_flagless_axes_exclude_archived() {
    let st = store();
    let u = "alice@x.com";
    st.upsert_thread(u, &row("live", 200, "inbox")).unwrap();
    st.upsert_thread(u, &row("gone", 100, "inbox")).unwrap();
    st.set_archived(u, "gone", true).unwrap();

    let (rows, total) = st
        .list_threads_by_activity(u, &ListThreadsFilter::default(), 0, 50)
        .unwrap();
    assert_eq!(tids(&rows), vec!["live"], "the default axis");
    assert_eq!(total, 1);

    let f = ListThreadsFilter {
        category: Some("inbox"),
        ..Default::default()
    };
    let (rows, total) = st.list_threads_by_activity(u, &f, 0, 50).unwrap();
    assert_eq!(tids(&rows), vec!["live"], "the category axis");
    assert_eq!(total, 1);
}

/// A flag keyed on its own index excludes it as well — that path reaches
/// `archived` as a value filter rather than through the sort prefix, so
/// it is a second implementation of the same rule.
#[test]
fn a_flag_axis_excludes_archived() {
    let st = store();
    let u = "alice@x.com";
    for tid in ["kept", "filed"] {
        st.upsert_thread(u, &row(tid, 100, "inbox")).unwrap();
        st.set_starred(u, tid, true).unwrap();
    }
    st.set_archived(u, "filed", true).unwrap();

    let f = ListThreadsFilter {
        folder: Some("nonjunk"),
        starred: true,
        ..Default::default()
    };
    let (rows, total) = st.list_threads_by_activity(u, &f, 0, 50).unwrap();
    assert_eq!(tids(&rows), vec!["kept"]);
    assert_eq!(total, 1);
}

/// The unread badge counts the set the Unread view lists.
///
/// `count_flag_non_junk` exists so those two are built by one function;
/// this pins the archive exclusion to that promise, because a badge
/// offering a number you cannot reach is how the last one went wrong.
#[test]
fn the_unread_badge_counts_what_the_unread_list_shows() {
    let st = store();
    let u = "alice@x.com";
    // Through the arrival path: `unread` is a per-user flag with its own
    // writer, and `upsert_thread` deliberately does not touch it.
    for tid in ["new", "filed"] {
        st.record_message_arrival(&crate::MessageArrival {
            thread_id: tid,
            user: u,
            subject: "s",
            senders_csv: "a@x.com",
            latest_date: 100,
            latest_preview: "p",
            category: "inbox",
            unread: true,
            is_own: false,
        })
        .unwrap();
    }
    st.set_archived(u, "filed", true).unwrap();

    let f = ListThreadsFilter {
        folder: Some("nonjunk"),
        has_unread: true,
        ..Default::default()
    };
    let (rows, _) = st.list_threads_by_activity(u, &f, 0, 50).unwrap();
    let badge = st.count_flag_non_junk(u, "unread").unwrap();

    assert_eq!(tids(&rows), vec!["new"]);
    assert_eq!(badge, rows.len(), "badge and list must agree");
}

/// A row written before the flags existed is still listed after boot.
///
/// `archived` joined every ORDERPATH prefix in the same change that
/// started excluding it, and a row missing a column an ORDERPATH keys on
/// is in none of them. The arrival path — which creates most rows — had
/// never written the per-user flags at all, so declaring the new shape
/// without migrating the rows first would have served every account an
/// empty mailbox. `ensure_thread_table` plants them before it declares;
/// this is that ordering, from the outside.
#[test]
fn a_row_written_before_the_flags_existed_is_listed_after_boot() {
    let u = "alice@x.com";
    let raw = Arc::new(Store::open(Config::default()).expect("open in-memory kevy"));

    // The row exactly as the old arrival path left it: every derived
    // column, no flags — and written **into the bare store**, because
    // `KevyMailboxStore::new` declares the table, and on a real boot the
    // AOF has been replayed by then. That ordering is the whole point:
    // the rows are there before the declaration reads them.
    let pairs = crate::thread_row::thread_user_pairs(u, &row("legacy", 100, "inbox"), None);
    let refs: Vec<(&[u8], &[u8])> = pairs
        .iter()
        .map(|(k, v)| (k.as_slice(), v.as_slice()))
        .collect();
    raw.hset(crate::keys::thread_user(u, "legacy").as_bytes(), &refs)
        .unwrap();

    let st = KevyMailboxStore::new(raw);

    let f = ListThreadsFilter {
        folder: Some("Inbox"),
        ..Default::default()
    };
    let (rows, total) = st.list_threads_by_activity(u, &f, 0, 50).unwrap();
    assert_eq!(tids(&rows), vec!["legacy"], "boot must not lose the row");
    assert_eq!(total, 1);
}

/// Paging is where the old shape did its real damage: the cursor is a
/// range on `activity`, and `archived` sits ahead of it in the prefix,
/// so an unpinned column would have made the page order a sort the index
/// does not hold.
#[test]
fn the_cursor_pages_the_live_list_only() {
    let st = store();
    let u = "alice@x.com";
    for i in 0..6i64 {
        st.upsert_thread(u, &row(&format!("t{i}"), 100 + i, "inbox"))
            .unwrap();
    }
    for tid in ["t4", "t2"] {
        st.set_archived(u, tid, true).unwrap();
    }

    let page = |before: Option<i64>| {
        let f = ListThreadsFilter {
            folder: Some("Inbox"),
            before_ts: before,
            ..Default::default()
        };
        st.list_threads_by_activity(u, &f, 0, 2).unwrap().0
    };

    let first = page(None);
    assert_eq!(tids(&first), vec!["t5", "t3"]);
    let cursor = first.last().unwrap().latest_date;
    let second = page(Some(cursor));
    assert_eq!(
        tids(&second),
        vec!["t1", "t0"],
        "the cursor must skip the archived rows rather than land on them"
    );
}
