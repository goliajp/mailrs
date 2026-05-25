//! Microbenchmarks for [`InMemoryMailboxStore`] hot-path ops.
//!
//! The trait is async, but the in-memory impl's RwLock + Vec backing makes
//! every operation a few microseconds at most. These benchmarks measure the
//! pure framework cost — the same ops backed by PostgreSQL pay round-trip
//! latency on top, which dominates per-call cost and is not bench-able here.
//!
//! Run with `cargo bench -p mailrs-mailbox`.

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};

use mailrs_mailbox::MailboxStore;
use mailrs_mailbox::fixtures::{EXAMPLE_USER, InMemoryMailboxStore};
use mailrs_mailbox::types::{FLAG_SEEN, InsertMessage, QueryFilter};

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn make_input<'a>(
    user: &'a str,
    mailbox: &'a str,
    subject: &'a str,
    mid: &'a str,
) -> InsertMessage<'a> {
    InsertMessage {
        user,
        mailbox_name: mailbox,
        blob_ref: "blob-ref",
        sender: "alice@example.com",
        recipients: "bob@example.com",
        subject,
        size: 4096,
        date: 1_715_000_000,
        internal_date: 1_715_000_001,
        message_id: mid,
        in_reply_to: "",
        thread_id: "t-1",
        flags: 0,
    }
}

async fn seed(store: &InMemoryMailboxStore, n: usize) {
    store.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    for i in 0..n {
        let subject = format!("Message {i}");
        let mid = format!("msg-{i}@example.com");
        let input = make_input(EXAMPLE_USER, "INBOX", &subject, &mid);
        store.insert_message(input).await.unwrap();
    }
}

fn bench_insert_message(c: &mut Criterion) {
    let rt = rt();
    let mut group = c.benchmark_group("insert_message");

    // Single insert into an empty mailbox.
    group.bench_function("first_insert", |b| {
        b.iter_batched(
            || {
                let store = InMemoryMailboxStore::new();
                rt.block_on(async {
                    store.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
                });
                store
            },
            |store| {
                rt.block_on(async {
                    let input = make_input(EXAMPLE_USER, "INBOX", "hi", "first@x");
                    let _ = store.insert_message(black_box(input)).await;
                })
            },
            BatchSize::SmallInput,
        )
    });

    // Insert into a mailbox that already has 1000 messages — exercises
    // the per-insert mailbox lookup over a non-trivial Vec.
    group.bench_function("insert_into_1k_mailbox", |b| {
        b.iter_batched(
            || {
                let store = InMemoryMailboxStore::new();
                rt.block_on(async {
                    seed(&store, 1000).await;
                });
                store
            },
            |store| {
                rt.block_on(async {
                    let input = make_input(EXAMPLE_USER, "INBOX", "n+1", "n+1@x");
                    let _ = store.insert_message(black_box(input)).await;
                })
            },
            BatchSize::LargeInput,
        )
    });

    group.finish();
}

fn bench_query_messages(c: &mut Criterion) {
    let rt = rt();
    let store = InMemoryMailboxStore::new();
    rt.block_on(async { seed(&store, 1000).await });

    let mut group = c.benchmark_group("query_messages");

    // Mailbox scope only, paginate first page.
    group.bench_function("by_mailbox_first_50", |b| {
        b.iter(|| {
            rt.block_on(async {
                let filter = QueryFilter {
                    mailbox_id: Some(1),
                    user: Some(EXAMPLE_USER),
                    text: None,
                    has_keyword: None,
                    not_keyword: None,
                    position: 0,
                    limit: 50,
                };
                store.query_messages(black_box(filter)).await
            })
        })
    });

    // Text substring scan across 1000 messages.
    group.bench_function("text_match_1k_messages", |b| {
        b.iter(|| {
            rt.block_on(async {
                let filter = QueryFilter {
                    mailbox_id: Some(1),
                    user: Some(EXAMPLE_USER),
                    text: Some("Message 7"),
                    has_keyword: None,
                    not_keyword: None,
                    position: 0,
                    limit: 50,
                };
                store.query_messages(black_box(filter)).await
            })
        })
    });

    // Flag filter (unread = !FLAG_SEEN). All seeded with flags=0 so all match.
    group.bench_function("not_keyword_seen_1k_messages", |b| {
        b.iter(|| {
            rt.block_on(async {
                let filter = QueryFilter {
                    mailbox_id: Some(1),
                    user: Some(EXAMPLE_USER),
                    text: None,
                    has_keyword: None,
                    not_keyword: Some(FLAG_SEEN),
                    position: 0,
                    limit: 50,
                };
                store.query_messages(black_box(filter)).await
            })
        })
    });

    group.finish();
}

fn bench_flag_ops(c: &mut Criterion) {
    let rt = rt();
    let mut group = c.benchmark_group("flag_ops");

    // add_flags on a hot mailbox.
    group.bench_function("add_flags_hot_path", |b| {
        // pre-build store + insert one message
        let store = InMemoryMailboxStore::new();
        rt.block_on(async {
            seed(&store, 1).await;
        });
        b.iter(|| {
            rt.block_on(async {
                let _ = store
                    .add_flags(black_box(1), black_box(1), black_box(FLAG_SEEN))
                    .await;
            })
        })
    });

    // CONDSTORE compare-and-swap; modseq is bumped each iter so the
    // unchangedsince check always succeeds (we pass current modseq).
    group.bench_function("store_flags_if_unchanged", |b| {
        let store = InMemoryMailboxStore::new();
        let inserted = rt.block_on(async {
            store.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
            let input = make_input(EXAMPLE_USER, "INBOX", "x", "x@x");
            store.insert_message(input).await.unwrap()
        });
        b.iter(|| {
            rt.block_on(async {
                let _ = store
                    .store_flags_if_unchanged(
                        black_box(1),
                        black_box(inserted.uid),
                        black_box(mailrs_mailbox::types::FlagOp::Add),
                        black_box(FLAG_SEEN),
                        black_box(u64::MAX),
                    )
                    .await;
            })
        })
    });

    group.finish();
}

fn bench_mailbox_status(c: &mut Criterion) {
    let rt = rt();
    let store = InMemoryMailboxStore::new();
    rt.block_on(async { seed(&store, 1000).await });

    c.bench_function("mailbox_status_1k_messages", |b| {
        b.iter(|| rt.block_on(async { store.mailbox_status(black_box(1)).await }))
    });
}

criterion_group!(
    benches,
    bench_insert_message,
    bench_query_messages,
    bench_flag_ops,
    bench_mailbox_status
);
criterion_main!(benches);
