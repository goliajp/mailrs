use criterion::{criterion_group, criterion_main, Criterion};
use mailrs_rfc2231::{decode_param_value, encode_param};
use std::hint::black_box;

fn bench_encode_ascii(c: &mut Criterion) {
    c.bench_function("encode/ascii_legacy_quoted", |b| {
        b.iter(|| {
            let r = encode_param(black_box("filename"), black_box("attachment.pdf"));
            black_box(r)
        });
    });
}

fn bench_encode_japanese(c: &mut Criterion) {
    c.bench_function("encode/japanese_extended", |b| {
        b.iter(|| {
            let r = encode_param(black_box("filename"), black_box("日本語ファイル.pdf"));
            black_box(r)
        });
    });
}

fn bench_encode_long_filename(c: &mut Criterion) {
    let long: String = "テスト".repeat(20); // ~60 Japanese chars = ~180 UTF-8 bytes
    c.bench_function("encode/long_japanese", |b| {
        b.iter(|| {
            let r = encode_param(black_box("filename"), black_box(&long));
            black_box(r)
        });
    });
}

fn bench_decode_quoted(c: &mut Criterion) {
    c.bench_function("decode/legacy_quoted", |b| {
        b.iter(|| {
            let r = decode_param_value(black_box("\"attachment.pdf\""));
            black_box(r)
        });
    });
}

fn bench_decode_bareword(c: &mut Criterion) {
    c.bench_function("decode/legacy_bareword", |b| {
        b.iter(|| {
            let r = decode_param_value(black_box("attachment"));
            black_box(r)
        });
    });
}

fn bench_decode_extended_utf8(c: &mut Criterion) {
    c.bench_function("decode/extended_utf8", |b| {
        b.iter(|| {
            let r = decode_param_value(black_box("UTF-8''%E6%97%A5%E6%9C%AC.pdf"));
            black_box(r)
        });
    });
}

fn bench_decode_extended_iso(c: &mut Criterion) {
    c.bench_function("decode/extended_iso_8859_1", |b| {
        b.iter(|| {
            let r = decode_param_value(black_box("iso-8859-1''caf%E9.txt"));
            black_box(r)
        });
    });
}

criterion_group!(
    benches,
    bench_encode_ascii,
    bench_encode_japanese,
    bench_encode_long_filename,
    bench_decode_quoted,
    bench_decode_bareword,
    bench_decode_extended_utf8,
    bench_decode_extended_iso,
);
criterion_main!(benches);
