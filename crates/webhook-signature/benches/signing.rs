use criterion::{criterion_group, criterion_main, Criterion};
use mailrs_webhook_signature::{format_header, parse_header, sign, verify, verify_any};
use std::hint::black_box;

fn bench_sign_short(c: &mut Criterion) {
    let secret = b"32-byte-shared-webhook-secret-aa";
    let payload = b"{\"event\":\"new_message\"}";
    c.bench_function("sign/short_payload_23_bytes", |b| {
        b.iter(|| {
            let r = sign(black_box(secret), black_box(payload));
            black_box(r)
        });
    });
}

fn bench_sign_1kb(c: &mut Criterion) {
    let secret = b"32-byte-shared-webhook-secret-aa";
    let payload: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
    c.bench_function("sign/1kb_payload", |b| {
        b.iter(|| {
            let r = sign(black_box(secret), black_box(&payload));
            black_box(r)
        });
    });
}

fn bench_sign_100kb(c: &mut Criterion) {
    let secret = b"32-byte-shared-webhook-secret-aa";
    let payload: Vec<u8> = (0..100_000).map(|i| (i % 256) as u8).collect();
    c.bench_function("sign/100kb_payload", |b| {
        b.iter(|| {
            let r = sign(black_box(secret), black_box(&payload));
            black_box(r)
        });
    });
}

fn bench_verify_correct(c: &mut Criterion) {
    let secret = b"32-byte-shared-webhook-secret-aa";
    let payload = b"{\"event\":\"new_message\"}";
    let sig = sign(secret, payload);
    c.bench_function("verify/correct_path", |b| {
        b.iter(|| {
            let r = verify(black_box(secret), black_box(payload), black_box(&sig));
            black_box(r)
        });
    });
}

fn bench_verify_wrong_secret(c: &mut Criterion) {
    let payload = b"{\"event\":\"new_message\"}";
    let sig = sign(b"right-secret", payload);
    c.bench_function("verify/wrong_secret_constant_time", |b| {
        b.iter(|| {
            let r = verify(black_box(b"wrong-secret"), black_box(payload), black_box(&sig));
            black_box(r)
        });
    });
}

fn bench_verify_any_first(c: &mut Criterion) {
    let payload = b"{\"event\":\"new_message\"}";
    let sig = sign(b"current-secret", payload);
    c.bench_function("verify_any/first_secret_matches", |b| {
        b.iter(|| {
            let secrets: &[&[u8]] = &[b"current-secret", b"previous-secret"];
            let r = verify_any(secrets, black_box(payload), black_box(&sig));
            black_box(r)
        });
    });
}

fn bench_verify_any_second(c: &mut Criterion) {
    let payload = b"{\"event\":\"new_message\"}";
    let sig = sign(b"previous-secret", payload);
    c.bench_function("verify_any/second_secret_matches", |b| {
        b.iter(|| {
            let secrets: &[&[u8]] = &[b"current-secret", b"previous-secret"];
            let r = verify_any(secrets, black_box(payload), black_box(&sig));
            black_box(r)
        });
    });
}

fn bench_format_header(c: &mut Criterion) {
    let sig = "a".repeat(64);
    c.bench_function("format_header", |b| {
        b.iter(|| {
            let r = format_header(black_box(&sig));
            black_box(r)
        });
    });
}

fn bench_parse_header_with_prefix(c: &mut Criterion) {
    let value = "sha256=abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    c.bench_function("parse_header_with_prefix", |b| {
        b.iter(|| {
            let r = parse_header(black_box(value));
            black_box(r)
        });
    });
}

criterion_group!(
    benches,
    bench_sign_short,
    bench_sign_1kb,
    bench_sign_100kb,
    bench_verify_correct,
    bench_verify_wrong_secret,
    bench_verify_any_first,
    bench_verify_any_second,
    bench_format_header,
    bench_parse_header_with_prefix,
);
criterion_main!(benches);
