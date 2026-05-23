//! Head-to-head: `mailrs-rate-limit` (in-memory token bucket) vs
//! `governor` 0.10, the de-facto Rust rate-limit crate (GCRA-based,
//! not strict token-bucket but in practice the comparison point).
//!
//! Comparison points (the only thing that matters in an SMTP/IMAP
//! frontline is the cost per `check`):
//!
//! * `hot_allowed` — warm key, allowed result. The realistic
//!   per-connection cost.
//! * `cold_first_touch` — key seen for the first time. Realistic
//!   churn cost (rotating client IPs, abuse fingerprints).
//!
//! Important: GCRA (governor) and token-bucket (us) are not exactly
//! the same algorithm, but for the purpose of "how much does it cost
//! to ask `is this request allowed?`", the comparison is direct.

use criterion::{Criterion, criterion_group, criterion_main};
use governor::{Quota, RateLimiter};
use mailrs_rate_limit::{InMemoryRateLimitStore, TokenBucketConfig};
use std::hint::black_box;
use std::num::NonZeroU32;

fn bench_hot_allowed(c: &mut Criterion) {
    let mut group = c.benchmark_group("hot_allowed");

    // mailrs-rate-limit warm path.
    let mailrs = InMemoryRateLimitStore::new(TokenBucketConfig {
        capacity: 1_000_000,
        refill_rate: 1_000_000.0,
    });
    let _ = mailrs.check_sync("hot-key");
    group.bench_function("mailrs_rate_limit", |b| {
        b.iter(|| mailrs.check_sync(black_box("hot-key")))
    });

    // governor warm path (keyed, dashmap-backed by default).
    let q = Quota::per_second(NonZeroU32::new(1_000_000).unwrap());
    let lim = RateLimiter::keyed(q);
    let key: &str = "hot-key";
    let _ = lim.check_key(&key);
    group.bench_function("governor", |b| {
        b.iter(|| {
            // Box the result so the discard cost is identical to ours.
            let r = lim.check_key(black_box(&key));
            black_box(r.is_ok())
        });
    });

    group.finish();
}

fn bench_cold_first_touch(c: &mut Criterion) {
    let mut group = c.benchmark_group("cold_first_touch");

    group.bench_function("mailrs_rate_limit", |b| {
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

    group.bench_function("governor", |b| {
        let q = Quota::per_second(NonZeroU32::new(10).unwrap());
        let lim: RateLimiter<String, _, _> = RateLimiter::keyed(q);
        let mut counter: u64 = 0;
        b.iter(|| {
            counter += 1;
            let key = format!("cold-{counter}");
            let r = lim.check_key(black_box(&key));
            black_box(r.is_ok())
        });
    });

    group.finish();
}

criterion_group!(benches, bench_hot_allowed, bench_cold_first_touch);
criterion_main!(benches);
