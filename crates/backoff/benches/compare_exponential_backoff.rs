//! Head-to-head: `mailrs-backoff` vs `exponential-backoff` 2.x.
//!
//! Both crates compute "given attempt N, what should the delay be?"
//! mailrs-backoff is a pure-function `base_delay(attempt)` plus an optional
//! deterministic jitter via `delay(attempt, seed)`.
//! `exponential-backoff` is iterator-shaped: build an iterator, advance it
//! attempt times to read the Nth delay.
//!
//! For an apples-to-apples comparison we measure:
//!
//! * Single-call cost to compute the delay for attempt 5 (mid-curve).
//! * Sequential read of 8 delays (a full retry chain).
//!
//! The iterator API in `exponential-backoff` is paid up front (creating the
//! iterator state); we include that in the per-call measurement to be fair —
//! a real caller has to pay it.

use criterion::{Criterion, criterion_group, criterion_main};
use exponential_backoff::Backoff as ExpBackoff;
use mailrs_backoff::{Backoff, Jitter};
use std::hint::black_box;
use std::time::Duration;

fn bench_single_attempt_no_jitter(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_attempt/no_jitter");

    let mailrs = Backoff::new(
        Duration::from_millis(100),
        2.0,
        Duration::from_secs(60),
        Jitter::None,
    );
    group.bench_function("mailrs_backoff", |b| {
        b.iter(|| black_box(mailrs.base_delay(black_box(5))))
    });

    group.bench_function("exponential_backoff", |b| {
        b.iter(|| {
            let backoff = ExpBackoff::new(8, Duration::from_millis(100), Duration::from_secs(60));
            // step to the 5th delay
            let mut it = backoff.iter();
            let mut last = None;
            for _ in 0..5 {
                last = it.next();
            }
            black_box(last)
        });
    });

    group.finish();
}

fn bench_single_attempt_full_jitter(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_attempt/full_jitter");

    let mailrs = Backoff::new(
        Duration::from_millis(100),
        2.0,
        Duration::from_secs(60),
        Jitter::Full,
    );
    group.bench_function("mailrs_backoff", |b| {
        // Deterministic — caller provides seed; we feed it from a counter
        let mut seed: u64 = 0;
        b.iter(|| {
            seed = seed.wrapping_add(1);
            black_box(mailrs.delay(black_box(5), black_box(seed)))
        });
    });

    group.bench_function("exponential_backoff", |b| {
        b.iter(|| {
            // exponential-backoff has its own jitter knob; same shape
            let mut backoff =
                ExpBackoff::new(8, Duration::from_millis(100), Duration::from_secs(60));
            backoff.set_jitter(0.5);
            let mut it = backoff.iter();
            let mut last = None;
            for _ in 0..5 {
                last = it.next();
            }
            black_box(last)
        });
    });

    group.finish();
}

fn bench_full_chain_8_attempts(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_chain_8_attempts/no_jitter");

    let mailrs = Backoff::new(
        Duration::from_millis(100),
        2.0,
        Duration::from_secs(60),
        Jitter::None,
    );
    group.bench_function("mailrs_backoff", |b| {
        b.iter(|| {
            let mut total = Duration::ZERO;
            for n in 0..8u32 {
                total += mailrs.base_delay(black_box(n));
            }
            black_box(total)
        });
    });

    group.bench_function("exponential_backoff", |b| {
        b.iter(|| {
            let backoff = ExpBackoff::new(8, Duration::from_millis(100), Duration::from_secs(60));
            let mut total = Duration::ZERO;
            for d in backoff.iter().flatten() {
                total += d;
            }
            black_box(total)
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_single_attempt_no_jitter,
    bench_single_attempt_full_jitter,
    bench_full_chain_8_attempts,
);
criterion_main!(benches);
