use crate::KevyMailboxStore;
use crate::list_threads::*;
use crate::thread_row::ThreadRow;
use kevy_embedded::{Config, Store};
use std::sync::Arc;

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

/// Every mutation that touches a thread must leave the membership
/// row agreeing with it.
///
/// The row is a second copy of facts the thread hash already
/// holds, and only `upsert_thread` used to maintain it — so any
/// path that wrote the hash directly (move_category, mark_seen,
/// the flag setters) silently desynchronised the axes that read
/// from the row. This walks each mutation and re-reads the row.
#[test]
fn every_mutation_keeps_the_membership_row_in_step() {
    let st = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    st.ensure_thread_table();
    let u = "alice@x.com";
    let mut seed = row("t", 100, "inbox");
    seed.unread_count = 2;
    st.upsert_thread(u, &seed).unwrap();

    let field = |name: &str| -> String {
        let key = crate::keys::thread_user(u, "t");
        st.store()
            .hgetall(key.as_bytes())
            .unwrap()
            .into_iter()
            .find(|(f, _)| f == name.as_bytes())
            .map(|(_, v)| String::from_utf8_lossy(&v).into_owned())
            .unwrap_or_default()
    };

    st.set_starred(u, "t", true).unwrap();
    assert_eq!(field("starred"), "1", "set_starred must update the row");

    st.set_archived(u, "t", true).unwrap();
    assert_eq!(field("archived"), "1", "set_archived must update the row");

    st.set_pinned(u, "t", true).unwrap();
    assert_eq!(field("pinned"), "1", "set_pinned must update the row");

    st.mark_seen(u, "t").unwrap();
    assert_eq!(field("unread"), "0", "mark_seen must update the row");

    st.move_category(u, "t", "spam").unwrap();
    assert_eq!(
        field("category"),
        "spam",
        "move_category must update the row"
    );
    assert_eq!(field("bucket"), "junk", "and the bucket derived from it");
}

/// Reclassification must remove the thread from its old category.
///
/// The zset this replaces never did: on prod one account's
/// `by_category:inbox` held 28598 entries against 6787 live rows,
/// because nothing deleted the old entry when a thread moved.
#[test]
fn reclassifying_leaves_the_old_category() {
    let st = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    st.ensure_thread_table();

    st.upsert_thread("alice@x.com", &row("t", 100, "inbox"))
        .unwrap();
    st.upsert_thread("alice@x.com", &row("t", 100, "spam"))
        .unwrap();

    let count = |cat: &str| {
        let f = ListThreadsFilter {
            category: Some(cat),
            ..Default::default()
        };
        st.list_threads_by_activity("alice@x.com", &f, 0, 10)
            .unwrap()
    };
    let (inbox, inbox_total) = count("inbox");
    assert!(
        inbox.is_empty(),
        "the old category must not keep the thread"
    );
    assert_eq!(inbox_total, 0, "nor keep counting it");
    let (spam, spam_total) = count("spam");
    assert_eq!(spam.len(), 1, "the new category must hold it");
    assert_eq!(spam_total, 1);
}

/// A flag stacked on a folder — the shape the UI produces every
/// time someone opens Archived, or filters Inbox by unread.
///
/// This class returned **nothing** for a day after the legacy
/// zsets were deleted: the bare paths did not match it and the
/// fallback was a ZINTERSTORE over indexes that no longer existed.
#[test]
fn a_flag_stacked_on_a_folder_is_served() {
    let st = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    st.ensure_thread_table();
    let u = "alice@x.com";

    st.upsert_thread(u, &row("ai", 300, "inbox")).unwrap();
    st.upsert_thread(u, &row("aj", 200, "spam")).unwrap();
    // Live inbox thread, not archived.
    st.upsert_thread(u, &row("live", 100, "inbox")).unwrap();
    // Archiving is a per-user act, so it goes through the mutator.
    for tid in ["ai", "aj"] {
        st.set_archived(u, tid, true).unwrap();
    }

    let archived_in = |folder: &'static str| {
        let f = ListThreadsFilter {
            folder: Some(folder),
            archived: true,
            ..Default::default()
        };
        st.list_threads_by_activity(u, &f, 0, 50).unwrap()
    };

    let (rows, total) = archived_in("Inbox");
    assert_eq!(
        rows.iter()
            .map(|r| r.thread_id.as_str())
            .collect::<Vec<_>>(),
        vec!["ai"],
        "archived-within-Inbox must return the archived inbox thread"
    );
    assert_eq!(total, 1, "and count it");

    let (rows, _) = archived_in("Junk");
    assert_eq!(
        rows.iter()
            .map(|r| r.thread_id.as_str())
            .collect::<Vec<_>>(),
        vec!["aj"],
        "the scope must actually scope"
    );
}

/// Unread and starred stack the same way.
#[test]
fn unread_and_starred_stack_on_a_folder_too() {
    let st = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    st.ensure_thread_table();
    let u = "alice@x.com";

    st.upsert_thread(u, &row("u1", 300, "inbox")).unwrap();
    st.upsert_thread(u, &row("s1", 200, "inbox")).unwrap();
    st.upsert_thread(u, &row("plain", 100, "inbox")).unwrap();
    st.mark_unread(u, "u1").unwrap();
    st.set_starred(u, "s1", true).unwrap();

    let ids = |f: ListThreadsFilter<'_>| {
        st.list_threads_by_activity(u, &f, 0, 50)
            .unwrap()
            .0
            .into_iter()
            .map(|r| r.thread_id)
            .collect::<Vec<_>>()
    };

    assert_eq!(
        ids(ListThreadsFilter {
            folder: Some("Inbox"),
            has_unread: true,
            ..Default::default()
        }),
        vec!["u1".to_string()]
    );
    assert_eq!(
        ids(ListThreadsFilter {
            folder: Some("Inbox"),
            starred: true,
            ..Default::default()
        }),
        vec!["s1".to_string()]
    );
}

/// The np view is the only axis whose order this code produces
/// rather than the engine — a two-way merge of two sorted sides.
/// Interleave the two buckets so a merge bug cannot hide behind
/// one side happening to be newer throughout.
#[test]
fn np_merges_both_buckets_by_recency() {
    let st = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    st.ensure_thread_table();
    let u = "alice@x.com";
    for (tid, when, cat) in [
        ("n1", 100, "notification"),
        ("p1", 150, "promotion"),
        ("n2", 200, "notification"),
        ("p2", 250, "promotion"),
    ] {
        st.upsert_thread(u, &row(tid, when, cat)).unwrap();
    }
    // Must not appear: neither bucket.
    st.upsert_thread(u, &row("inb", 999, "inbox")).unwrap();

    let f = ListThreadsFilter {
        folder: Some("np"),
        ..Default::default()
    };
    let (rows, total) = st.list_threads_by_activity(u, &f, 0, 10).unwrap();
    assert_eq!(
        rows.iter()
            .map(|r| r.thread_id.as_str())
            .collect::<Vec<_>>(),
        vec!["p2", "n2", "p1", "n1"],
        "the two buckets must interleave by recency"
    );
    assert_eq!(total, 4);

    // And paging through the merge must stay contiguous.
    let (p1, _) = st.list_threads_by_activity(u, &f, 0, 2).unwrap();
    let (p2, _) = st.list_threads_by_activity(u, &f, 2, 2).unwrap();
    assert_eq!(
        p1.iter()
            .chain(p2.iter())
            .map(|r| r.thread_id.as_str())
            .collect::<Vec<_>>(),
        vec!["p2", "n2", "p1", "n1"],
        "offset paging across a merge must not repeat or skip"
    );
}

/// Sent is the sent_only flag, and it is the complement of what
/// the inbox axis excludes.
#[test]
fn sent_axis_holds_only_sent_only_threads() {
    let st = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    st.ensure_thread_table();
    let u = "alice@x.com";

    let mut mine = row("mine", 200, "inbox");
    mine.senders_csv = u.into();
    mine.sent_count = mine.count;
    st.upsert_thread(u, &mine).unwrap();

    let mut replied = row("replied", 100, "inbox");
    replied.senders_csv = format!("bob@y.com,{u}");
    replied.count = 3;
    replied.sent_count = 1;
    st.upsert_thread(u, &replied).unwrap();

    // Never written in — must not be in Sent.
    st.upsert_thread(u, &row("theirs", 50, "inbox")).unwrap();

    let f = ListThreadsFilter {
        folder: Some("Sent"),
        ..Default::default()
    };
    let (rows, total) = st.list_threads_by_activity(u, &f, 0, 10).unwrap();
    assert_eq!(
        rows.iter()
            .map(|r| r.thread_id.as_str())
            .collect::<Vec<_>>(),
        vec!["mine", "replied"],
        "Sent holds every thread the user wrote in, replies included"
    );
    assert_eq!(total, 2);
}

/// Each bucket is a partition: a thread lands in exactly one of
/// them, so every axis must exclude the other three.
#[test]
fn bucket_axes_partition_the_threads() {
    let st = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    st.ensure_thread_table();

    for (tid, cat) in [
        ("i", "inbox"),
        ("n", "notification"),
        ("p", "promotion"),
        ("j", "spam"),
    ] {
        st.upsert_thread("alice@x.com", &row(tid, 100, cat))
            .unwrap();
    }

    for (folder, want) in [
        ("Inbox", "i"),
        ("Notifications", "n"),
        ("Promotions", "p"),
        ("Junk", "j"),
    ] {
        let filter = ListThreadsFilter {
            folder: Some(folder),
            ..Default::default()
        };
        let (rows, total) = st
            .list_threads_by_activity("alice@x.com", &filter, 0, 10)
            .unwrap();
        assert_eq!(
            rows.iter()
                .map(|r| r.thread_id.as_str())
                .collect::<Vec<_>>(),
            vec![want],
            "{folder} must hold exactly its own bucket"
        );
        assert_eq!(total, 1, "{folder} count must exclude the other buckets");
    }
}

/// The unread badge and the unread view must describe the same set.
///
/// They did not. `unseen_count` counted the unread flag across every
/// bucket while the Unread tab asked for `folder=Inbox`, so a mailbox
/// whose only unread mail was promotions showed "2 Unread" on the
/// dashboard and an empty list everywhere that number led. The scope
/// exists so the two can be stated once; this pins that they agree.
#[test]
fn the_non_junk_scope_is_exactly_what_the_unread_badge_counts() {
    let st = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    st.ensure_thread_table();
    let u = "alice@x.com";
    for (tid, when, cat) in [
        ("i1", 100, "inbox"),
        ("p1", 200, "promotion"),
        ("n1", 300, "notification"),
        ("j1", 400, "spam"),
    ] {
        st.upsert_thread(u, &row(tid, when, cat)).unwrap();
        // `unread` is a per-user column, and only its own mutator
        // writes it — setting `unread_count` on the thread would leave
        // the membership row the flag index reads at zero.
        st.mark_unread(u, tid).unwrap();
    }

    let f = ListThreadsFilter {
        folder: Some("nonjunk"),
        has_unread: true,
        ..Default::default()
    };
    let (rows, total) = st.list_threads_by_activity(u, &f, 0, 10).unwrap();
    assert_eq!(
        rows.iter()
            .map(|r| r.thread_id.as_str())
            .collect::<Vec<_>>(),
        vec!["n1", "p1", "i1"],
        "three buckets merged by recency, and junk is not one of them"
    );
    assert_eq!(total, 3);

    // The number `unseen_count` serves, from the function it calls.
    let badge = st.count_flag_non_junk(u, "unread").unwrap();
    assert_eq!(badge, total, "the badge must count what the view lists");
}

/// A thread the user only ever sent into is not in their Inbox, and the
/// merged scope must not be the side door that puts it back.
#[test]
fn the_non_junk_scope_excludes_sent_only_threads_from_its_inbox_side() {
    let st = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    st.ensure_thread_table();
    let u = "alice@x.com";
    // "Sent-only" said as the row can still say it: every sender in the
    // thread is this user. It used to be `count == sent_count`, and
    // those fields are no longer written — the counters are derived by
    // the aggregate index, and the fallback for a thread the index
    // cannot see reads `senders_csv`.
    let mut sent = row("s1", 100, "inbox");
    sent.senders_csv = u.to_string();
    st.upsert_thread(u, &sent).unwrap();
    let received = row("r1", 200, "inbox");
    st.upsert_thread(u, &received).unwrap();

    let f = ListThreadsFilter {
        folder: Some("nonjunk"),
        ..Default::default()
    };
    let (rows, total) = st.list_threads_by_activity(u, &f, 0, 10).unwrap();
    assert_eq!(
        rows.iter()
            .map(|r| r.thread_id.as_str())
            .collect::<Vec<_>>(),
        vec!["r1"],
        "sent-only threads stay out, exactly as folder=Inbox has them"
    );
    assert_eq!(total, 1);
}
