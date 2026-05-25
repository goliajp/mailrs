//! Micro-benchmarks for maildir flag parsing.
//!
//! Run with: `cargo bench -p mailrs-maildir`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_maildir::{Flag, add_flag, parse_flags, serialize_flags};

fn bench_parse_flags(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_flags");
    group.bench_function("empty", |b| b.iter(|| parse_flags(black_box(""))));
    group.bench_function("seen_only", |b| b.iter(|| parse_flags(black_box("S"))));
    group.bench_function("all_standard", |b| {
        b.iter(|| parse_flags(black_box("FRPST")))
    });
    group.bench_function("with_garbage", |b| {
        b.iter(|| parse_flags(black_box("FXSXTX")))
    });
    group.finish();
}

fn bench_serialize_flags(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialize_flags");
    let empty: Vec<Flag> = vec![];
    group.bench_function("empty", |b| b.iter(|| serialize_flags(black_box(&empty))));

    let all = vec![
        Flag::Flagged,
        Flag::Replied,
        Flag::Passed,
        Flag::Seen,
        Flag::Trashed,
    ];
    group.bench_function("all_standard", |b| {
        b.iter(|| serialize_flags(black_box(&all)))
    });
    group.finish();
}

fn bench_add_flag(c: &mut Criterion) {
    c.bench_function("add_flag_to_existing", |b| {
        b.iter(|| add_flag(black_box("FR"), black_box(Flag::Seen)))
    });
}

criterion_group!(
    benches,
    bench_parse_flags,
    bench_serialize_flags,
    bench_add_flag
);
criterion_main!(benches);
