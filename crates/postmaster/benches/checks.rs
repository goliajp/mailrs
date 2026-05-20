//! Microbenchmarks for the pure helpers (no live resolver hits).

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_postmaster::extract_bimi_logo_url;

fn bench_bimi_extract(c: &mut Criterion) {
    let record = "v=BIMI1; l=https://example.com/logo.svg; a=https://example.com/cert.pem";
    c.bench_function("extract_bimi_logo_url", |b| {
        b.iter(|| extract_bimi_logo_url(black_box(record)))
    });
}

criterion_group!(benches, bench_bimi_extract);
criterion_main!(benches);
