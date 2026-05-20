//! Micro-benchmarks for smtp-client hot paths.
//!
//! Run with: `cargo bench -p mailrs-smtp-client`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_smtp_client::connection::dot_stuff;
use mailrs_smtp_client::mx::{MxRecord, fallback_to_domain, sort_mx_records};
use mailrs_smtp_client::response::parse_response;

const SHORT_EHLO: &str = "250 OK\r\n";

const LONG_EHLO: &str = "\
250-smtp.example.com Hello [192.0.2.1]\r\n\
250-SIZE 36700160\r\n\
250-STARTTLS\r\n\
250-8BITMIME\r\n\
250-PIPELINING\r\n\
250-AUTH PLAIN LOGIN\r\n\
250-CHUNKING\r\n\
250-DSN\r\n\
250-SMTPUTF8\r\n\
250 ENHANCEDSTATUSCODES\r\n";

fn bench_parse_response(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_response");
    group.bench_function("short", |b| b.iter(|| parse_response(black_box(SHORT_EHLO))));
    group.bench_function("long_ehlo_10_lines", |b| {
        b.iter(|| parse_response(black_box(LONG_EHLO)))
    });
    group.finish();
}

fn bench_dot_stuff(c: &mut Criterion) {
    let body_with_dots = format!(
        "From: a@x\r\nTo: b@x\r\nSubject: dots\r\n\r\n{}",
        ".dot at start of every other line\r\nnormal line\r\n".repeat(50)
    );
    let body_no_dots = format!(
        "From: a@x\r\nTo: b@x\r\nSubject: no dots\r\n\r\n{}",
        "ordinary content here\r\nmore content\r\n".repeat(50)
    );

    let mut group = c.benchmark_group("dot_stuff");
    group.bench_function("body_no_dots", |b| {
        b.iter(|| dot_stuff(black_box(body_no_dots.as_bytes())))
    });
    group.bench_function("body_with_dots", |b| {
        b.iter(|| dot_stuff(black_box(body_with_dots.as_bytes())))
    });
    group.finish();
}

fn bench_mx_sort(c: &mut Criterion) {
    let mut records: Vec<MxRecord> = (0..20)
        .map(|i| MxRecord {
            exchange: format!("mx{i}.example.com"),
            priority: (i * 7 + 13) % 100,
        })
        .collect();

    c.bench_function("sort_mx_records_n20", |b| {
        b.iter(|| sort_mx_records(black_box(&mut records)))
    });

    c.bench_function("fallback_to_domain", |b| {
        b.iter(|| fallback_to_domain(black_box("example.com")))
    });
}

criterion_group!(benches, bench_parse_response, bench_dot_stuff, bench_mx_sort);
criterion_main!(benches);
