use criterion::{criterion_group, criterion_main, Criterion};
use mailrs_srs::{reverse, rewrite, DEFAULT_TIMESTAMP_WINDOW_DAYS};
use std::hint::black_box;

fn bench_rewrite(c: &mut Criterion) {
    c.bench_function("rewrite/ascii_sender", |b| {
        b.iter(|| {
            let r = rewrite(
                black_box("alice@example.com"),
                black_box("mx.golia.jp"),
                black_box("shared-secret-key"),
            );
            black_box(r)
        });
    });
}

fn bench_reverse_success(c: &mut Criterion) {
    let secret = "shared-secret-key";
    let rewritten = rewrite("alice@example.com", "mx.golia.jp", secret);
    c.bench_function("reverse/success_path", |b| {
        b.iter(|| {
            let r = reverse(
                black_box(&rewritten),
                black_box(secret),
                DEFAULT_TIMESTAMP_WINDOW_DAYS,
            );
            black_box(r)
        });
    });
}

fn bench_reverse_wrong_secret(c: &mut Criterion) {
    let rewritten = rewrite("alice@example.com", "mx.golia.jp", "right-secret");
    c.bench_function("reverse/wrong_secret_constant_time", |b| {
        b.iter(|| {
            let r = reverse(
                black_box(&rewritten),
                black_box("wrong-secret"),
                DEFAULT_TIMESTAMP_WINDOW_DAYS,
            );
            black_box(r)
        });
    });
}

fn bench_reverse_malformed(c: &mut Criterion) {
    c.bench_function("reverse/malformed_input", |b| {
        b.iter(|| {
            let r = reverse(
                black_box("not-an-srs-address@example"),
                black_box("secret"),
                DEFAULT_TIMESTAMP_WINDOW_DAYS,
            );
            black_box(r)
        });
    });
}

criterion_group!(
    benches,
    bench_rewrite,
    bench_reverse_success,
    bench_reverse_wrong_secret,
    bench_reverse_malformed,
);
criterion_main!(benches);
