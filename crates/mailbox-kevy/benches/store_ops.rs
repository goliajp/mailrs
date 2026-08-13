//! Per-operation cost of the store production actually runs on.
//!
//! This crate had no benchmark and no perf gate until 2026-08-13, which is
//! why the last kevy upgrade shipped with no per-op numbers at all —
//! `PERFORMANCE.md` says so in as many words: *"Not benched separately:
//! throughput on individual embedded ops."* Forty gates and fifty-two
//! benchmarks in this workspace, and the two store-shaped ones ran against
//! an in-memory `Vec`.
//!
//! Run with `cargo bench -p mailrs-mailbox-kevy`.
//!
//! ## Why the fixture is 23,508 threads
//!
//! Because that is what `scripts/bench-api-seed.py` builds, and the two
//! measurements have to be about the same store or neither explains the
//! other. It also matters for what is being measured: a declared ORDERPATH
//! at three rows tells you the cost of the call, not the cost of the index,
//! and the failure modes worth catching here — a scan where a seek was
//! declared, a count that walks — only appear with a keyspace under them.
//!
//! The store is in memory (`Config::default()`, no `with_persist`), so
//! nothing here pays fsync. That is deliberate: this file answers "what does
//! the op cost", and the durability floor is a property of the disk, which
//! `scripts/bench-api-e2e.sh` measures end to end instead.
//!
//! ## Two of these exist to watch a property, not a speed
//!
//! `backfill_thread_user/converged` measures the periodic sweep in the state
//! it spends almost all its life in: nothing to repair. `periodic-work-must-
//! converge` asks that this be *cheap*, not merely idempotent — and the two
//! are different, which is the whole lesson. It carries an assertion as well
//! as a timing, because the sweep's own `written` counter can falsify the
//! property outright where a duration can only hint at it.
//!
//! `allocate_uid/repeat` is the same shape for the dedupe path: the second
//! call for a message-id must not do the work of the first.
//!
//! Everything here goes through the crate's public surface. The convergent
//! write itself is `pub(crate)`, and a bench is another crate — widening it
//! to be measurable would trade a real boundary for a number, so the sweep
//! that calls it is measured instead. That is also the more useful subject:
//! the sweep is what runs on the timer.

use std::hint::black_box;
use std::sync::Arc;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};

use kevy_embedded::{Config, Store};
use mailrs_mailbox_kevy::{
    KevyMailboxStore, ListThreadsFilter, MessageArrival, ThreadRow, UserMessageFacts,
};

const USER: &str = "bench@bench.local";
/// Matches `scripts/bench-api-seed.py`'s THREADS.
const THREADS: i64 = 23_508;
const CATEGORIES: [&str; 4] = ["inbox", "notification", "promotion", "general"];

fn row(tid: &str, activity: i64, category: &str) -> ThreadRow {
    ThreadRow {
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

/// A populated store: `THREADS` membership rows for one user, spread over
/// the four categories, one in ten starred and one in five unread, plus a
/// second user's rows so every read has something it must exclude.
fn populated() -> KevyMailboxStore {
    let st = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    st.ensure_thread_table();

    for i in 1..=THREADS {
        let tid = format!("thread-{i}");
        let r = row(&tid, 1_748_000_000 + i * 60, CATEGORIES[(i % 4) as usize]);
        st.upsert_thread(USER, &r)
            .expect("seed thread + membership row");
        // Per-user flags go through the per-user mutators, because
        // `upsert_thread` deliberately plants them at zero on a row it has
        // just created: starred and friends belong to an owner, and the
        // shared aggregate's copy is not authoritative for a new
        // membership row.
        //
        // Setting them on the ThreadRow instead — the first version of this
        // fixture — left every flag index empty, and the flag-keyed page
        // then benchmarked at 551 ns because it was returning nothing. An
        // empty answer is fast and measures nothing, which is why the reads
        // below assert before they time.
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
    // Another owner's copy of one thread. Every read below must leave it
    // out, and a read that stopped scoping by user would still look fast.
    st.upsert_thread(
        "other@bench.local",
        &row("thread-1", 1_748_000_060, "inbox"),
    )
    .expect("seed second owner");
    st
}

/// Time nothing until it is known to be something.
///
/// A read that returns an empty page is the fastest read there is, and a
/// bench of one reports a number that looks like a win. The whole fixture
/// was wrong once in exactly this way, so every shape asserts a full page
/// before it is measured.
fn assert_full_page(st: &KevyMailboxStore, label: &str, f: &ListThreadsFilter<'_>, want: usize) {
    let (rows, total) = st
        .list_threads_by_activity(USER, f, 0, want)
        .expect("list_threads");
    assert_eq!(
        rows.len(),
        want,
        "{label} returned {} of {want} rows (index total {total}) — a bench over \
         a short page measures the wrong thing",
        rows.len()
    );
}

fn reads(c: &mut Criterion) {
    let st = populated();
    let mut g = c.benchmark_group("list_threads");
    // One page, the size the web client asks for.
    let limit = 50;

    // The four declared ORDERPATHs, one bench each, named for the shape the
    // dispatcher picks rather than for the path — the mapping is the thing
    // that breaks.
    assert_full_page(&st, "all/page1", &ListThreadsFilter::default(), limit);
    g.bench_function("all/page1", |b| {
        let f = ListThreadsFilter::default();
        b.iter(|| black_box(st.list_threads_by_activity(USER, &f, 0, limit).unwrap()));
    });
    assert_full_page(
        &st,
        "bucket_inbox/page1",
        &ListThreadsFilter {
            folder: Some("inbox"),
            ..Default::default()
        },
        limit,
    );
    g.bench_function("bucket_inbox/page1", |b| {
        let f = ListThreadsFilter {
            folder: Some("inbox"),
            ..Default::default()
        };
        b.iter(|| black_box(st.list_threads_by_activity(USER, &f, 0, limit).unwrap()));
    });
    assert_full_page(
        &st,
        "category/page1",
        &ListThreadsFilter {
            category: Some("promotion"),
            ..Default::default()
        },
        limit,
    );
    g.bench_function("category/page1", |b| {
        let f = ListThreadsFilter {
            category: Some("promotion"),
            ..Default::default()
        };
        b.iter(|| black_box(st.list_threads_by_activity(USER, &f, 0, limit).unwrap()));
    });
    assert_full_page(
        &st,
        "starred_flag/page1",
        &ListThreadsFilter {
            starred: true,
            ..Default::default()
        },
        limit,
    );
    g.bench_function("starred_flag/page1", |b| {
        let f = ListThreadsFilter {
            starred: true,
            ..Default::default()
        };
        b.iter(|| black_box(st.list_threads_by_activity(USER, &f, 0, limit).unwrap()));
    });

    // Deep into the keyspace. A declared seek and a scan are the same price
    // on page one and not on page two hundred, which is the whole reason the
    // ORDERPATHs exist.
    g.bench_function("all/page200", |b| {
        let f = ListThreadsFilter::default();
        b.iter(|| {
            black_box(
                st.list_threads_by_activity(USER, &f, 200 * limit, limit)
                    .unwrap(),
            )
        });
    });
    g.finish();

    assert!(
        st.count_flag_non_junk(USER, "unread").unwrap() > 0
            && st.count_flag_non_junk(USER, "starred").unwrap() > 0,
        "a count of zero is not a count worth timing"
    );
    let mut g = c.benchmark_group("counts");
    // The unread badge. It shares its predicates with the page above so the
    // two cannot disagree; that also means it shares their cost profile.
    g.bench_function("unread_badge", |b| {
        b.iter(|| black_box(st.count_flag_non_junk(USER, "unread").unwrap()));
    });
    g.bench_function("starred", |b| {
        b.iter(|| black_box(st.count_flag_non_junk(USER, "starred").unwrap()));
    });
    g.finish();

    assert!(
        st.get_thread_for_user(USER, "thread-12345")
            .unwrap()
            .is_some(),
        "the probe thread must exist or this times a miss"
    );
    c.bench_function("get_thread_for_user", |b| {
        b.iter(|| black_box(st.get_thread_for_user(USER, "thread-12345").unwrap()));
    });
}

fn writes(c: &mut Criterion) {
    let st = populated();

    // The periodic sweep, on a store that has nothing left to repair.
    //
    // This is the instrument for `periodic-work-must-converge`: the sweep's
    // stable state has to be *cheap*, not merely idempotent. Overwriting a
    // value with the one already there is idempotent and is not convergent,
    // and the 2026-07-19 incident — 48,613 files re-read every 31 seconds,
    // logging `sent_added=255 created=0` the whole time — is what the
    // difference costs.
    //
    // The `written` counter is what makes the property falsifiable rather
    // than inferred from a duration, so it is asserted here: a converged
    // page must report zero writes. A sweep that reported work on an
    // already-correct store is the defect, whatever it costs.
    let mut g = c.benchmark_group("backfill_thread_user");
    {
        let (scanned, written) = st.backfill_thread_user(USER, 0, 200).unwrap();
        assert!(
            scanned > 0,
            "the fixture should give the sweep something to walk"
        );
        let (_, again) = st.backfill_thread_user(USER, 0, 200).unwrap();
        assert_eq!(
            again, 0,
            "a second pass over an unchanged page wrote {again} rows — the sweep \
             is idempotent but not convergent, which is the shape that burned \
             a core for hours on 2026-07-19 (first pass wrote {written})"
        );
    }
    g.bench_function("converged/200", |b| {
        b.iter(|| black_box(st.backfill_thread_user(USER, 0, 200).unwrap()));
    });
    g.finish();

    c.bench_function("upsert_thread", |b| {
        let r = row("thread-99", 1_748_006_000, "inbox");
        // No black_box: these return `()`, and there is no value to keep
        // alive. The call mutates the store through an Arc, so it cannot be
        // elided either way.
        b.iter(|| st.upsert_thread(USER, &r).unwrap());
    });

    c.bench_function("set_thread_importance", |b| {
        b.iter(|| {
            st.set_thread_importance(USER, "thread-100", "high", 0.9)
                .unwrap()
        });
    });

    c.bench_function("mark_seen", |b| {
        // Each iteration needs a thread that is unread, so re-arm it in the
        // untimed setup rather than measuring the already-seen path.
        b.iter_batched(
            || {
                let mut r = row("thread-101", 1_748_006_060, "inbox");
                r.unread_count = 1;
                st.upsert_thread(USER, &r).unwrap();
            },
            |()| black_box(st.mark_seen(USER, "thread-101").unwrap()),
            BatchSize::SmallInput,
        );
    });

    let mut g = c.benchmark_group("allocate_uid");
    g.bench_function("fresh", |b| {
        let mut n = 0u64;
        b.iter(|| {
            n += 1;
            black_box(
                st.allocate_uid(USER, &format!("fresh-{n}@example.com"))
                    .unwrap(),
            )
        });
    });
    g.bench_function("repeat", |b| {
        // The dedupe path: already allocated, must not redo the work.
        st.allocate_uid(USER, "settled@example.com").unwrap();
        b.iter(|| black_box(st.allocate_uid(USER, "settled@example.com").unwrap()));
    });
    g.finish();

    c.bench_function("deliver_message", |b| {
        let payload = vec![b'x'; 2048];
        let mut n = 0u64;
        b.iter(|| {
            n += 1;
            let mid = format!("deliver-{n}@example.com");
            let tid = format!("thread-deliver-{n}");
            let arrival = MessageArrival {
                thread_id: &tid,
                user: USER,
                subject: "quarterly forecast renewal",
                senders_csv: "sender3@example3.com",
                latest_date: 1_748_100_000 + n as i64,
                latest_preview: "deadline budget review",
                category: "inbox",
                unread: true,
                is_own: false,
            };
            st.deliver_message(
                &arrival,
                &mid,
                &payload,
                &UserMessageFacts {
                    blob_ref: "maildir-bench",
                    uid: 0,
                    flags: 0,
                    modseq: n,
                },
            )
            .unwrap();
        });
    });
}

criterion_group!(benches, reads, writes);
criterion_main!(benches);
