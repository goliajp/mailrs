//! Micro-benchmarks for the smtp-proto hot paths.
//!
//! Run with: `cargo bench -p mailrs-smtp-proto`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_smtp_proto::address::{is_valid, split_address};
use mailrs_smtp_proto::parse::parse_command;
use mailrs_smtp_proto::response::format_ehlo_response;

fn bench_parse_command(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_command");
    group.bench_function("EHLO", |b| {
        b.iter(|| parse_command(black_box("EHLO mail.example.com\r\n")))
    });
    group.bench_function("MAIL_FROM", |b| {
        b.iter(|| parse_command(black_box("MAIL FROM:<alice@example.com> SIZE=12345\r\n")))
    });
    group.bench_function("RCPT_TO", |b| {
        b.iter(|| parse_command(black_box("RCPT TO:<bob@example.com>\r\n")))
    });
    group.bench_function("DATA", |b| b.iter(|| parse_command(black_box("DATA\r\n"))));
    group.bench_function("AUTH_PLAIN", |b| {
        b.iter(|| parse_command(black_box("AUTH PLAIN AGFsaWNlAHBhc3N3b3Jk\r\n")))
    });
    group.finish();
}

fn bench_address(c: &mut Criterion) {
    let mut group = c.benchmark_group("address");
    group.bench_function("is_valid_typical", |b| {
        b.iter(|| is_valid(black_box("alice.smith+work@example.co.jp")))
    });
    group.bench_function("split_typical", |b| {
        b.iter(|| split_address(black_box("alice.smith+work@example.co.jp")))
    });
    group.finish();
}

fn bench_format_ehlo(c: &mut Criterion) {
    let caps = [
        "SIZE 36700160",
        "STARTTLS",
        "8BITMIME",
        "PIPELINING",
        "AUTH PLAIN LOGIN",
        "DSN",
    ];
    c.bench_function("format_ehlo_response", |b| {
        b.iter(|| format_ehlo_response(black_box("mail.example.com"), black_box(&caps)))
    });
}

criterion_group!(
    benches,
    bench_parse_command,
    bench_address,
    bench_format_ehlo
);
criterion_main!(benches);
