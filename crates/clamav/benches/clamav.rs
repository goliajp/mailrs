use criterion::{criterion_group, criterion_main, Criterion};
use mailrs_clamav::{parse_response, ClamavResult};
use std::hint::black_box;

fn bench_parse_clean(c: &mut Criterion) {
    c.bench_function("parse_response/clean", |b| {
        b.iter(|| {
            let r = parse_response(black_box(b"stream: OK\n"));
            assert!(matches!(r, ClamavResult::Clean));
            black_box(r)
        });
    });
}

fn bench_parse_clean_with_nul(c: &mut Criterion) {
    c.bench_function("parse_response/clean_with_nul", |b| {
        b.iter(|| {
            let r = parse_response(black_box(b"stream: OK\0"));
            black_box(r)
        });
    });
}

fn bench_parse_virus_short_name(c: &mut Criterion) {
    c.bench_function("parse_response/virus_short_name", |b| {
        b.iter(|| {
            let r = parse_response(black_box(b"stream: Eicar FOUND\n"));
            black_box(r)
        });
    });
}

fn bench_parse_virus_long_name(c: &mut Criterion) {
    c.bench_function("parse_response/virus_long_name", |b| {
        b.iter(|| {
            let r = parse_response(black_box(
                b"stream: Trojan.Win32.Generic-7654321.UltraSafe FOUND\n",
            ));
            black_box(r)
        });
    });
}

fn bench_parse_error(c: &mut Criterion) {
    c.bench_function("parse_response/error_size_limit", |b| {
        b.iter(|| {
            let r = parse_response(black_box(b"INSTREAM size limit exceeded. ERROR"));
            black_box(r)
        });
    });
}

fn bench_parse_empty(c: &mut Criterion) {
    c.bench_function("parse_response/empty", |b| {
        b.iter(|| {
            let r = parse_response(black_box(b""));
            black_box(r)
        });
    });
}

criterion_group!(
    benches,
    bench_parse_clean,
    bench_parse_clean_with_nul,
    bench_parse_virus_short_name,
    bench_parse_virus_long_name,
    bench_parse_error,
    bench_parse_empty,
);
criterion_main!(benches);
