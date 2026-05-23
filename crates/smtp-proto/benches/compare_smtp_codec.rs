//! Head-to-head: `mailrs-smtp-proto::parse_command` vs `smtp-codec` 0.2.
//!
//! smtp-codec is a nom-based SMTP parser/serializer. Both parse a wire-
//! format command line into a structured representation. We measure the
//! most common commands.

use criterion::{Criterion, criterion_group, criterion_main};
use mailrs_smtp_proto::parse_command;
use std::hint::black_box;

const EHLO: &[u8] = b"EHLO mail.example.com\r\n";
const MAIL_FROM: &[u8] = b"MAIL FROM:<alice@example.com> SIZE=10240\r\n";
const RCPT_TO: &[u8] = b"RCPT TO:<bob@example.com>\r\n";
const DATA: &[u8] = b"DATA\r\n";

fn bench_ehlo(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse/ehlo");
    group.bench_function("mailrs_smtp_proto", |b| {
        let s = std::str::from_utf8(EHLO).unwrap().trim_end_matches("\r\n");
        b.iter(|| {
            let r = parse_command(black_box(s));
            black_box(r.unwrap())
        });
    });
    group.bench_function("smtp_codec", |b| {
        b.iter(|| {
            let r = smtp_codec::parse::command::command(black_box(EHLO));
            black_box(r.ok())
        });
    });
    group.finish();
}

fn bench_mail_from(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse/mail_from");
    group.bench_function("mailrs_smtp_proto", |b| {
        let s = std::str::from_utf8(MAIL_FROM)
            .unwrap()
            .trim_end_matches("\r\n");
        b.iter(|| {
            let r = parse_command(black_box(s));
            black_box(r.unwrap())
        });
    });
    group.bench_function("smtp_codec", |b| {
        b.iter(|| {
            let r = smtp_codec::parse::command::command(black_box(MAIL_FROM));
            black_box(r.ok())
        });
    });
    group.finish();
}

fn bench_rcpt_to(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse/rcpt_to");
    group.bench_function("mailrs_smtp_proto", |b| {
        let s = std::str::from_utf8(RCPT_TO).unwrap().trim_end_matches("\r\n");
        b.iter(|| {
            let r = parse_command(black_box(s));
            black_box(r.unwrap())
        });
    });
    group.bench_function("smtp_codec", |b| {
        b.iter(|| {
            let r = smtp_codec::parse::command::command(black_box(RCPT_TO));
            black_box(r.ok())
        });
    });
    group.finish();
}

fn bench_data(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse/data");
    group.bench_function("mailrs_smtp_proto", |b| {
        let s = std::str::from_utf8(DATA).unwrap().trim_end_matches("\r\n");
        b.iter(|| {
            let r = parse_command(black_box(s));
            black_box(r.unwrap())
        });
    });
    group.bench_function("smtp_codec", |b| {
        b.iter(|| {
            let r = smtp_codec::parse::command::command(black_box(DATA));
            black_box(r.ok())
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_ehlo,
    bench_mail_from,
    bench_rcpt_to,
    bench_data,
);
criterion_main!(benches);
