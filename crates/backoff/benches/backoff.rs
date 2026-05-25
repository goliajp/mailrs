use criterion::{Criterion, criterion_group, criterion_main};
use mailrs_backoff::{Backoff, Jitter};
use std::hint::black_box;
use std::time::Duration;

fn bench_base_delay(c: &mut Criterion) {
    let b = Backoff::smtp_outbound();
    c.bench_function("base_delay/attempt_3", |bench| {
        bench.iter(|| {
            let d = b.base_delay(black_box(3));
            black_box(d)
        });
    });
}

fn bench_delay_none(c: &mut Criterion) {
    let b = Backoff {
        initial: Duration::from_secs(60),
        multiplier: 2.0,
        max: Duration::from_secs(3600),
        jitter: Jitter::None,
    };
    c.bench_function("delay/none_jitter", |bench| {
        bench.iter(|| {
            let d = b.delay(black_box(3), black_box(42));
            black_box(d)
        });
    });
}

fn bench_delay_equal(c: &mut Criterion) {
    let b = Backoff::webhook();
    c.bench_function("delay/equal_jitter", |bench| {
        bench.iter(|| {
            let d = b.delay(black_box(3), black_box(42));
            black_box(d)
        });
    });
}

fn bench_delay_full(c: &mut Criterion) {
    let b = Backoff::smtp_outbound();
    c.bench_function("delay/full_jitter", |bench| {
        bench.iter(|| {
            let d = b.delay(black_box(3), black_box(42));
            black_box(d)
        });
    });
}

fn bench_should_give_up(c: &mut Criterion) {
    c.bench_function("should_give_up", |b| {
        b.iter(|| {
            let r = Backoff::should_give_up(black_box(5), black_box(10));
            black_box(r)
        });
    });
}

fn bench_delay_high_attempt_capped(c: &mut Criterion) {
    let b = Backoff::smtp_outbound();
    c.bench_function("delay/high_attempt_capped", |bench| {
        bench.iter(|| {
            let d = b.delay(black_box(100), black_box(42));
            black_box(d)
        });
    });
}

criterion_group!(
    benches,
    bench_base_delay,
    bench_delay_none,
    bench_delay_equal,
    bench_delay_full,
    bench_should_give_up,
    bench_delay_high_attempt_capped,
);
criterion_main!(benches);
