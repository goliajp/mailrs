use crate::KevyMailboxStore;
use crate::list_threads::*;
use kevy_embedded::{Config, Store};
use std::sync::Arc;

fn store() -> KevyMailboxStore {
    let s = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    // Reads are served from the declared table, so a test store
    // has to look like a booted one.
    s.ensure_thread_table();
    s
}

fn row(tid: &str, date: i64, category: &str) -> ThreadRow {
    ThreadRow {
        account_id: String::new(),
        thread_id: tid.into(),
        subject: format!("subject of {tid}"),
        senders_csv: "x@y.z".into(),
        count: 1,
        unread_count: 0,
        latest_date: date,
        latest_preview: "".into(),
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

#[test]
fn lists_in_reverse_activity_order() {
    let s = store();
    let u = "u@x.com";
    // out-of-order insertion
    s.upsert_thread(u, &row("t2", 200, "inbox")).unwrap();
    s.upsert_thread(u, &row("t1", 100, "inbox")).unwrap();
    s.upsert_thread(u, &row("t3", 300, "inbox")).unwrap();
    let (got, total) = s
        .list_threads_by_activity(u, &ListThreadsFilter::default(), 0, 10)
        .unwrap();
    assert_eq!(total, 3);
    let tids: Vec<&str> = got.iter().map(|r| r.thread_id.as_str()).collect();
    assert_eq!(tids, vec!["t3", "t2", "t1"]); // highest date first
}

#[test]
fn offset_and_limit_paginate() {
    let s = store();
    let u = "u@x.com";
    for i in 0..10 {
        s.upsert_thread(u, &row(&format!("t{i}"), i as i64, "inbox"))
            .unwrap();
    }
    let (got, total) = s
        .list_threads_by_activity(u, &ListThreadsFilter::default(), 3, 4)
        .unwrap();
    assert_eq!(total, 10);
    let tids: Vec<&str> = got.iter().map(|r| r.thread_id.as_str()).collect();
    // reverse activity: t9 t8 t7 [t6 t5 t4 t3] t2 t1 t0
    assert_eq!(tids, vec!["t6", "t5", "t4", "t3"]);
}

#[test]
fn category_filter_uses_per_category_index() {
    let s = store();
    // the category axis is served from the declared table
    s.ensure_thread_table();
    let u = "u@x.com";
    s.upsert_thread(u, &row("a1", 100, "inbox")).unwrap();
    s.upsert_thread(u, &row("a2", 200, "social")).unwrap();
    s.upsert_thread(u, &row("a3", 300, "inbox")).unwrap();
    let f = ListThreadsFilter {
        category: Some("social"),
        ..Default::default()
    };
    let (got, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
    assert_eq!(total, 1);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].thread_id, "a2");
}

#[test]
fn pinned_filter_returns_only_pinned() {
    let s = store();
    // the flag axes are served from the declared table
    s.ensure_thread_table();
    let u = "u@x.com";
    s.upsert_thread(u, &row("p1", 100, "inbox")).unwrap();
    s.upsert_thread(u, &row("p2", 200, "inbox")).unwrap();
    s.set_pinned(u, "p1", true).unwrap();
    let f = ListThreadsFilter {
        pinned: true,
        ..Default::default()
    };
    let (got, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
    assert_eq!(total, 1);
    assert_eq!(got[0].thread_id, "p1");
}

/// Two flags at once is an intersection, not an empty page.
///
/// This is the shape that had no declared path: `bare_flag` returned
/// `None` for two, `stacked_predicate` returned `None` for two, and
/// what caught it was the zset intersection — over zsets that hold 0
/// rows on every prod account. The tab answered `[]` with a 200.
#[test]
fn two_flags_at_once_intersect() {
    let s = store();
    let u = "u@x.com";
    for (tid, when) in [("both", 300), ("star", 200), ("unread", 100)] {
        s.upsert_thread(u, &row(tid, when, "inbox")).unwrap();
    }
    // Through the mutators, which is where per-user state is set.
    for tid in ["both", "star"] {
        s.set_starred(u, tid, true).unwrap();
    }
    for tid in ["both", "unread"] {
        s.mark_unread(u, tid).unwrap();
    }

    let f = ListThreadsFilter {
        starred: true,
        has_unread: true,
        ..Default::default()
    };
    let (got, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
    assert_eq!(total, 1, "starred ∩ unread is one thread, not none");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].thread_id, "both");
}

/// Two flags *and* a folder — the same shape with a scope on top.
#[test]
fn two_flags_within_a_folder_stay_inside_it() {
    let s = store();
    let u = "u@x.com";
    s.upsert_thread(u, &row("in", 300, "inbox")).unwrap();
    s.upsert_thread(u, &row("junk", 400, "spam")).unwrap();
    for tid in ["in", "junk"] {
        s.set_starred(u, tid, true).unwrap();
        s.mark_unread(u, tid).unwrap();
    }

    let f = ListThreadsFilter {
        folder: Some("inbox"),
        starred: true,
        has_unread: true,
        ..Default::default()
    };
    let (got, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
    assert_eq!(total, 1);
    assert_eq!(got[0].thread_id, "in");
}

/// A folder and a category name the same axis twice. The category is
/// the narrower of the two, and the page is its page — not empty,
/// which is what having no path for the pair used to produce.
#[test]
fn a_folder_and_an_agreeing_category_use_the_narrower_one() {
    let s = store();
    let u = "u@x.com";
    s.upsert_thread(u, &row("social", 300, "social")).unwrap();
    s.upsert_thread(u, &row("plain", 200, "inbox")).unwrap();

    let f = ListThreadsFilter {
        folder: Some("inbox"),
        category: Some("social"),
        ..Default::default()
    };
    let (got, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
    assert_eq!(total, 1, "`social` sits in the inbox bucket");
    assert_eq!(got[0].thread_id, "social");
}

/// A folder and a category that cannot both hold is genuinely empty,
/// and says so in one place rather than by falling off the end.
#[test]
fn a_folder_and_a_disagreeing_category_are_empty() {
    let s = store();
    let u = "u@x.com";
    s.upsert_thread(u, &row("promo", 300, "promotion")).unwrap();
    s.upsert_thread(u, &row("spam", 200, "spam")).unwrap();

    let f = ListThreadsFilter {
        folder: Some("junk"),
        category: Some("promotion"),
        ..Default::default()
    };
    let (got, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
    assert_eq!(total, 0);
    assert!(got.is_empty());
}

/// The merged Notifications+Promotions view with a flag on it: two
/// ranges, keyed on the flag, merged by recency.
#[test]
fn np_with_a_flag_merges_both_buckets() {
    let s = store();
    let u = "u@x.com";
    for (tid, when, cat) in [
        ("n1", 100, "notification"),
        ("p1", 300, "promotion"),
        ("p2", 400, "promotion"),
        ("i1", 500, "inbox"),
    ] {
        s.upsert_thread(u, &row(tid, when, cat)).unwrap();
    }
    for tid in ["n1", "p1", "i1"] {
        s.set_starred(u, tid, true).unwrap();
    }

    let f = ListThreadsFilter {
        folder: Some("np"),
        starred: true,
        ..Default::default()
    };
    let (got, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
    assert_eq!(total, 2, "starred within notifications ∪ promotions");
    let tids: Vec<&str> = got.iter().map(|r| r.thread_id.as_str()).collect();
    assert_eq!(tids, vec!["p1", "n1"], "merged newest-first");
}

/// Sent is a flag, so a predicate stacks on it like any other.
#[test]
fn sent_takes_a_stacked_flag() {
    let s = store();
    let u = "u@x.com";
    let mut sent_unread = row("s1", 300, "inbox");
    sent_unread.senders_csv = u.into();
    let mut sent_read = row("s2", 200, "inbox");
    sent_read.senders_csv = u.into();
    s.upsert_thread(u, &sent_unread).unwrap();
    s.upsert_thread(u, &sent_read).unwrap();
    s.mark_unread(u, "s1").unwrap();

    let f = ListThreadsFilter {
        folder: Some("sent"),
        has_unread: true,
        ..Default::default()
    };
    let (got, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
    assert_eq!(total, 1);
    assert_eq!(got[0].thread_id, "s1");
}

#[test]
fn cursor_paginates_by_date() {
    let s = store();
    let u = "u@x.com";
    // 5 threads at dates 100, 200, 300, 400, 500
    for i in 1..=5 {
        s.upsert_thread(u, &row(&format!("t{i}"), i * 100, "inbox"))
            .unwrap();
    }
    // First page — no cursor, limit 2.
    let (page1, _total) = s
        .list_threads_by_activity(u, &ListThreadsFilter::default(), 0, 2)
        .unwrap();
    assert_eq!(
        page1
            .iter()
            .map(|r| r.thread_id.as_str())
            .collect::<Vec<_>>(),
        vec!["t5", "t4"]
    );

    // Second page — cursor = last item's latest_date = 400. Should
    // return threads STRICTLY less than 400: t3 (300), t2 (200).
    let f = ListThreadsFilter {
        before_ts: Some(400),
        ..Default::default()
    };
    let (page2, _total) = s.list_threads_by_activity(u, &f, 0, 2).unwrap();
    assert_eq!(
        page2
            .iter()
            .map(|r| r.thread_id.as_str())
            .collect::<Vec<_>>(),
        vec!["t3", "t2"]
    );
}

#[test]
fn cursor_skips_ts_boundary() {
    let s = store();
    let u = "u@x.com";
    s.upsert_thread(u, &row("boundary", 500, "inbox")).unwrap();
    s.upsert_thread(u, &row("under", 499, "inbox")).unwrap();
    let f = ListThreadsFilter {
        before_ts: Some(500),
        ..Default::default()
    };
    let (rows, _total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].thread_id, "under");
}

#[test]
fn folder_sent_returns_only_sent_threads() {
    let s = store();
    let u = "u@x.com";
    // Sent membership is decided by senders_csv containing the user.
    let mut sent = row("s1", 200, "inbox");
    sent.senders_csv = "me <u@x.com>".into();
    let received = row("r1", 300, "inbox");
    s.upsert_thread(u, &sent).unwrap();
    s.upsert_thread(u, &received).unwrap();
    let f = ListThreadsFilter {
        folder: Some("Sent"),
        ..Default::default()
    };
    let (rows, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
    assert_eq!(total, 1);
    assert_eq!(rows[0].thread_id, "s1");

    // Case-insensitive match.
    let f2 = ListThreadsFilter {
        folder: Some("sent"),
        ..Default::default()
    };
    let (rows2, _) = s.list_threads_by_activity(u, &f2, 0, 10).unwrap();
    assert_eq!(rows2.len(), 1);
}

#[test]
fn offset_past_end_returns_empty() {
    let s = store();
    let u = "u@x.com";
    s.upsert_thread(u, &row("only", 1, "inbox")).unwrap();
    let (got, total) = s
        .list_threads_by_activity(u, &ListThreadsFilter::default(), 5, 10)
        .unwrap();
    assert_eq!(total, 1);
    assert!(got.is_empty());
}
