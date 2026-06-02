//! Micro-benchmarks for the smtp-proto hot paths.
//!
//! Run with: `cargo bench -p mailrs-smtp-proto`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_smtp_proto::address::{is_valid, split_address};
use mailrs_smtp_proto::data::unstuff_data;
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

fn bench_unstuff_data(c: &mut Criterion) {
    // `unstuff_data` runs on every inbound SMTP DATA payload.
    // Typical messages have one CRLF per ~80 ASCII chars; we bench at
    // 1 KB / 10 KB / 100 KB to span signature-size up to bulk email.
    let mut group = c.benchmark_group("unstuff_data");
    for &n in &[1_024usize, 10_240, 102_400] {
        let mut payload = Vec::with_capacity(n + 5);
        while payload.len() < n {
            payload.extend_from_slice(b"plain message body line, no dot-stuffing needed.\r\n");
        }
        payload.truncate(n);
        payload.extend_from_slice(b".\r\n");
        group.throughput(criterion::Throughput::Bytes(payload.len() as u64));
        group.bench_function(format!("{}b", n), |b| {
            b.iter(|| {
                let r = unstuff_data(black_box(&payload));
                black_box(r);
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_parse_command,
    bench_address,
    bench_format_ehlo,
    bench_unstuff_data
);
criterion_main!(benches);
