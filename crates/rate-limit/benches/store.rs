//! Microbenchmarks for the rate-limit hot path.
//!
//! Every inbound connection takes one `check` before any protocol work.
//! `evaluate_bucket` is the pure math underneath; the in-memory store wraps
//! it in a DashMap entry-lock + a `SystemTime::now()` syscall.
//!
//! Run with `cargo bench -p mailrs-rate-limit`.

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};

use mailrs_rate_limit::{
    Bucket, InMemoryRateLimitStore, RateLimitStore, TokenBucketConfig, evaluate_bucket,
};

fn bench_evaluate_bucket(c: &mut Criterion) {
    let config = TokenBucketConfig {
        capacity: 10,
        refill_rate: 1.0,
    };
    let bucket = Bucket {
        tokens: 5.0,
        last_refill_unix_secs: 1_000_000,
    };

    let mut group = c.benchmark_group("evaluate_bucket");
    group.bench_function("allowed", |b| {
        b.iter(|| evaluate_bucket(black_box(bucket), black_box(1_000_001), black_box(&config)))
    });

    let empty = Bucket {
        tokens: 0.0,
        last_refill_unix_secs: 1_000_000,
    };
    group.bench_function("denied_no_refill", |b| {
        b.iter(|| evaluate_bucket(black_box(empty), black_box(1_000_000), black_box(&config)))
    });
    group.finish();
}

fn bench_check_hot(c: &mut Criterion) {
    let store = InMemoryRateLimitStore::new(TokenBucketConfig {
        capacity: 1_000_000,
        refill_rate: 1_000_000.0,
    });
    // warm the entry — bench the "key already present" path
    let _ = store.check_sync("hot-key");

    let mut group = c.benchmark_group("check_hot_key");
    group.bench_function("sync", |b| {
        b.iter(|| store.check_sync(black_box("hot-key")))
    });

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    group.bench_function("async", |b| {
        b.iter(|| rt.block_on(async { store.check(black_box("hot-key")).await }))
    });
    group.finish();
}

fn bench_check_miss(c: &mut Criterion) {
    // Each iteration inserts a fresh key — measures the first-touch
    // alloc + DashMap insert path.
    let mut group = c.benchmark_group("check_cold_key");
    group.bench_function("first_touch", |b| {
        let store = InMemoryRateLimitStore::new(TokenBucketConfig {
            capacity: 10,
            refill_rate: 1.0,
        });
        let mut counter: u64 = 0;
        b.iter(|| {
            counter += 1;
            let key = format!("cold-{counter}");
            store.check_sync(black_box(&key))
        });
    });
    group.finish();
}

fn bench_cleanup_10k(c: &mut Criterion) {
    // Populate with 10k entries; sweep them all (cutoff in far future).
    let mut group = c.benchmark_group("cleanup_stale");

    group.bench_function("cleanup_10k_all_stale", |b| {
        b.iter_batched(
            || {
                let store = InMemoryRateLimitStore::new(TokenBucketConfig {
                    capacity: 1,
                    refill_rate: 0.0,
                });
                for i in 0..10_000 {
                    let _ = store.check_sync(&format!("k-{i}"));
                }
                store
            },
            |store| {
                // cutoff = u64::MAX → every bucket is stale
                store.cleanup_stale_sync(u64::MAX);
                black_box(store.len_sync())
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("cleanup_10k_none_stale", |b| {
        b.iter_batched(
            || {
                let store = InMemoryRateLimitStore::new(TokenBucketConfig {
                    capacity: 1,
                    refill_rate: 0.0,
                });
                for i in 0..10_000 {
                    let _ = store.check_sync(&format!("k-{i}"));
                }
                store
            },
            |store| {
                // cutoff = 0 → every bucket is fresh (last_refill ≥ 0)
                store.cleanup_stale_sync(0);
                black_box(store.len_sync())
            },
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_evaluate_bucket,
    bench_check_hot,
    bench_check_miss,
    bench_cleanup_10k
);
criterion_main!(benches);
