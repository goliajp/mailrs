use criterion::{criterion_group, criterion_main, Criterion};
use mailrs_smtp_codec::{has_smuggle_sequence, normalize_line_endings};
use std::hint::black_box;

fn bench_smuggle(c: &mut Criterion) {
    let safe = b"hello\r\n.\r\n";
    c.bench_function("has_smuggle_sequence/safe", |b| {
        b.iter(|| {
            let _ = has_smuggle_sequence(black_box(safe));
        });
    });
}

fn bench_normalize(c: &mut Criterion) {
    let lf_only = b"hello\nworld\n";
    c.bench_function("normalize_line_endings/lf_only", |b| {
        b.iter(|| {
            let _ = normalize_line_endings(black_box(lf_only));
        });
    });
}

criterion_group!(benches, bench_smuggle, bench_normalize);
criterion_main!(benches);
