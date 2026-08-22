//! Performance regression gates. See [BUDGETS.md](../BUDGETS.md).
//!
//! Release only. Every budget here was derived from an optimised build, and
//! asserting one in a dev build measures host contention instead of code
//! speed — twenty files in this workspace learned that on 2026-07-29, when
//! dkim and mime went red inside `cargo test --workspace` while passing ten
//! of ten in isolation.
//!
//! ## One test, on purpose
//!
//! Every gate runs inside a single `#[test]`, sequentially, against one
//! store. The first version of this file used ten test functions and they
//! measured each other: `cargo test` runs them on separate threads, they
//! shared the fixture through a `OnceLock`, and the write-heavy ones held
//! kevy's shard locks while the read-heavy ones were being timed. Same
//! operations, same build, 34–48× apart from the criterion figures —
//! `all/page1` came out at 5.30 ms against a measured 157 µs, and
//! `allocate_uid/fresh` at 6.63 µs against 163 ns.
//!
//! That is the failure the repo already has two instances of on file, so it
//! is worth naming: **a perf gate that shares mutable state across parallel
//! tests measures contention.** Collapsing to one test removes the sharing
//! rather than tuning the budgets around it.
//!
//! Violations are collected and reported together instead of failing at the
//! first one, because the useful output of a regression is the whole panel.

#![cfg(not(debug_assertions))]

use std::sync::Arc;
use std::time::{Duration, Instant};

use kevy_embedded::{Config, Store};
use mailrs_mailbox_kevy::{
    KevyMailboxStore, ListThreadsFilter, MessageArrival, ThreadRow, UserMessageFacts,
};

const ITERS: usize = 100;
const USER: &str = "bench@bench.local";
/// Matches `scripts/bench-api-seed.py` and `benches/store_ops.rs`. The three
/// have to agree or none of them explains the others.
const THREADS: i64 = 23_508;
const CATEGORIES: [&str; 4] = ["inbox", "notification", "promotion", "general"];

fn time_median<F: FnMut()>(mut op: F) -> Duration {
    let mut samples = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let start = Instant::now();
        op();
        samples.push(start.elapsed());
    }
    samples.sort();
    samples[ITERS / 2]
}

fn row(tid: &str, activity: i64, category: &str) -> ThreadRow {
    ThreadRow {
        account_id: String::new(),
        thread_id: tid.into(),
        subject: "quarterly forecast renewal approval ticket".into(),
        senders_csv: "sender7@example0.com".into(),
        count: 1,
        unread_count: 0,
        latest_date: activity,
        latest_preview: "deadline budget review contract shipment".into(),
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

fn populated() -> KevyMailboxStore {
    let st = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    st.ensure_thread_table();
    for i in 1..=THREADS {
        let tid = format!("thread-{i}");
        let r = row(&tid, 1_748_000_000 + i * 60, CATEGORIES[(i % 4) as usize]);
        st.upsert_thread(USER, &r).expect("seed row");
        // Through the per-user mutators, not the ThreadRow: `upsert_thread`
        // plants per-user flags at zero on a row it has just created, so
        // setting them on the aggregate leaves every flag index empty — and a
        // flag page over an empty index is fast and measures nothing. That
        // was the first fixture's bug, and `full_page` is the standing guard.
        if i % 10 == 0 {
            st.set_starred(USER, &tid, true).expect("seed starred");
        }
        if i % 5 == 0 {
            st.mark_unread(USER, &tid).expect("seed unread");
        }
        if i % 25 == 0 {
            st.set_has_action(USER, &tid, true)
                .expect("seed has_action");
        }
    }
    st
}

/// Collects what blew its budget, so one regression does not hide the rest.
struct Panel {
    over: Vec<String>,
    rows: Vec<(String, Duration, Duration)>,
}

impl Panel {
    fn new() -> Self {
        Self {
            over: Vec::new(),
            rows: Vec::new(),
        }
    }

    fn gate(&mut self, label: &str, median: Duration, budget: Duration) {
        self.rows.push((label.to_string(), median, budget));
        if median >= budget {
            self.over
                .push(format!("{label}: median {median:?} exceeded {budget:?}"));
        }
    }

    fn finish(self) {
        for (label, median, budget) in &self.rows {
            println!("{label:<28} {median:>12?}   budget {budget:?}");
        }
        assert!(
            self.over.is_empty(),
            "{} budget(s) exceeded:\n  {}",
            self.over.len(),
            self.over.join("\n  ")
        );
    }
}

/// Assert a shape answers a full page before it is timed.
fn full_page(st: &KevyMailboxStore, label: &str, f: &ListThreadsFilter<'_>, want: usize) {
    let (rows, total) = st
        .list_threads_by_activity(USER, f, 0, want)
        .expect("list_threads");
    assert_eq!(
        rows.len(),
        want,
        "{label} answered {} of {want} rows (index total {total}) — an empty \
         answer is the fastest one there is",
        rows.len()
    );
}

#[test]
fn store_ops_within_budget() {
    let st = populated();
    let mut p = Panel::new();

    // ── reads ───────────────────────────────────────────────────────────
    let all = ListThreadsFilter::default();
    full_page(&st, "all/page1", &all, 50);
    p.gate(
        "list/all/page1",
        time_median(|| {
            let _ = st.list_threads_by_activity(USER, &all, 0, 50).unwrap();
        }),
        Duration::from_micros(800),
    );

    let inbox = ListThreadsFilter {
        folder: Some("inbox"),
        ..Default::default()
    };
    full_page(&st, "bucket_inbox/page1", &inbox, 50);
    p.gate(
        "list/bucket_inbox/page1",
        time_median(|| {
            let _ = st.list_threads_by_activity(USER, &inbox, 0, 50).unwrap();
        }),
        Duration::from_micros(600),
    );

    let cat = ListThreadsFilter {
        category: Some("promotion"),
        ..Default::default()
    };
    full_page(&st, "category/page1", &cat, 50);
    p.gate(
        "list/category/page1",
        time_median(|| {
            let _ = st.list_threads_by_activity(USER, &cat, 0, 50).unwrap();
        }),
        Duration::from_micros(600),
    );

    let starred = ListThreadsFilter {
        starred: true,
        ..Default::default()
    };
    full_page(&st, "starred_flag/page1", &starred, 50);
    p.gate(
        "list/starred_flag/page1",
        time_median(|| {
            let _ = st.list_threads_by_activity(USER, &starred, 0, 50).unwrap();
        }),
        Duration::from_millis(6),
    );

    // Page 200 against page 1 is what says whether the declared path is a
    // seek or a walk.
    p.gate(
        "list/all/page200",
        time_median(|| {
            let _ = st
                .list_threads_by_activity(USER, &all, 200 * 50, 50)
                .unwrap();
        }),
        Duration::from_millis(5),
    );

    let unread = st.count_flag_non_junk(USER, "unread").unwrap();
    let n_starred = st.count_flag_non_junk(USER, "starred").unwrap();
    assert!(
        unread > 0 && n_starred > 0,
        "counts of {unread} unread / {n_starred} starred — a zero is not a count worth timing"
    );
    // The unread badge, on every conversation-list load. Its size is the
    // reason `idx_count_claused` is on the kevy 5.1 harvest list.
    p.gate(
        "count/unread_badge",
        time_median(|| {
            let _ = st.count_flag_non_junk(USER, "unread").unwrap();
        }),
        Duration::from_millis(16),
    );
    p.gate(
        "count/starred",
        time_median(|| {
            let _ = st.count_flag_non_junk(USER, "starred").unwrap();
        }),
        Duration::from_millis(7),
    );

    assert!(
        st.get_thread_for_user(USER, "thread-12345")
            .unwrap()
            .is_some(),
        "the probe thread must exist or this times a miss"
    );
    p.gate(
        "get_thread_for_user",
        time_median(|| {
            let _ = st.get_thread_for_user(USER, "thread-12345").unwrap();
        }),
        Duration::from_micros(10),
    );

    // ── the sweep's stable state ────────────────────────────────────────
    let (scanned, first) = st.backfill_thread_user(USER, 0, 200).unwrap();
    assert!(scanned > 0, "the sweep should have rows to walk");
    let (_, again) = st.backfill_thread_user(USER, 0, 200).unwrap();
    // The property, not the speed. `periodic-work-must-converge` asks for a
    // stable state that is cheap, and a write counter is the only thing that
    // can falsify it: overwriting a value with the one already there is
    // idempotent and is not convergent, which cost a core hours on
    // 2026-07-19 while logging `sent_added=255 created=0` every 31 seconds.
    assert_eq!(
        again, 0,
        "a second pass over an unchanged page wrote {again} rows (first pass \
         wrote {first}) — idempotent but not convergent"
    );
    // Converged is not free: the read half is still paid per row every pass.
    p.gate(
        "backfill/converged/200",
        time_median(|| {
            let _ = st.backfill_thread_user(USER, 0, 200).unwrap();
        }),
        Duration::from_millis(26),
    );

    // ── writes ──────────────────────────────────────────────────────────
    let r = row("thread-gate-write", 1_748_900_000, "inbox");
    p.gate(
        "upsert_thread",
        time_median(|| st.upsert_thread(USER, &r).unwrap()),
        Duration::from_micros(100),
    );
    p.gate(
        "set_thread_importance",
        time_median(|| {
            st.set_thread_importance(USER, "thread-gate-write", "high", 0.9)
                .unwrap()
        }),
        Duration::from_micros(80),
    );

    // 15 µs, from this gate's own 3.58 µs. Criterion reports 163 ns for the
    // same call and the two have not been reconciled — see BUDGETS.md. Every
    // other row here agrees with criterion to within a few percent, so the
    // budget follows the instrument that runs in CI rather than the one that
    // disagrees with it.
    let mut n = 0u64;
    p.gate(
        "allocate_uid/fresh",
        time_median(|| {
            n += 1;
            let _ = st
                .allocate_uid(USER, &format!("gate-fresh-{n}@example.com"))
                .unwrap();
        }),
        Duration::from_micros(15),
    );
    let settled = st.allocate_uid(USER, "gate-settled@example.com").unwrap();
    p.gate(
        "allocate_uid/repeat",
        time_median(|| {
            // The dedupe path has to give the same answer, not just a fast
            // one — a repeat that redid the work would pass a duration
            // budget on its own.
            let again = st.allocate_uid(USER, "gate-settled@example.com").unwrap();
            assert_eq!(again, settled);
        }),
        Duration::from_micros(2),
    );

    // The main ingest write: thread hash, membership row, per-user message
    // row, uid index, blob and the text index, in one call. A fresh thread
    // each time, like the seed's mostly single-message threads — re-using
    // one id grew it without bound and the number drifted upward while it
    // was being measured.
    let payload = vec![b'x'; 2048];
    let mut m = 0u64;
    p.gate(
        "deliver_message",
        time_median(|| {
            m += 1;
            let tid = format!("thread-gate-deliver-{m}");
            let arrival = MessageArrival {
                thread_id: &tid,
                user: USER,
                subject: "quarterly forecast renewal",
                senders_csv: "sender3@example3.com",
                latest_date: 1_748_900_000 + m as i64,
                latest_preview: "deadline budget review",
                category: "inbox",
                unread: true,
                is_own: false,
            };
            st.deliver_message(
                &arrival,
                &format!("gate-deliver-{m}@example.com"),
                &payload,
                &UserMessageFacts {
                    blob_ref: "maildir-gate",
                    uid: 0,
                    flags: 0,
                    modseq: m,
                },
            )
            .unwrap();
        }),
        Duration::from_micros(400),
    );

    p.finish();
}
