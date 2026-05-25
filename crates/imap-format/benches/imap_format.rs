use criterion::{Criterion, criterion_group, criterion_main};
use mailrs_imap_format::{format_imap_flags, format_internal_date, parse_imap_flags};
use std::hint::black_box;

fn bench_format(c: &mut Criterion) {
    c.bench_function("format_imap_flags/seen+answered", |b| {
        b.iter(|| {
            let _ = format_imap_flags(black_box(0b11));
        });
    });
    c.bench_function("parse_imap_flags/seen answered", |b| {
        b.iter(|| {
            let _ = parse_imap_flags(black_box("\\Seen \\Answered"));
        });
    });
    c.bench_function("format_internal_date", |b| {
        b.iter(|| {
            let _ = format_internal_date(black_box(1_700_000_000));
        });
    });
}

criterion_group!(benches, bench_format);
criterion_main!(benches);
