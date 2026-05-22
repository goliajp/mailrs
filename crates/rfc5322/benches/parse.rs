//! Comparative bench: `mailrs-rfc5322` vs `mail-parser` for the
//! common "read a few headers + body" pattern an inbound SMTP server
//! executes per message.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use mailrs_rfc5322::Message;
use std::hint::black_box;

fn build_sample(body_kb: usize) -> Vec<u8> {
    let mut msg = Vec::with_capacity(1024 + body_kb * 1024);
    msg.extend_from_slice(
        b"Return-Path: <alice@example.com>\r\n\
          Received: from mta.example.com (mta.example.com [203.0.113.42])\r\n\
              \tby mx.golia.jp with ESMTP id 12345; Sun, 22 May 2026 10:00:00 +0900\r\n\
          Received: from internal.example.com\r\n\
              \tby mta.example.com; Sun, 22 May 2026 09:59:50 +0900\r\n\
          From: \"Alice Liddell\" <alice@example.com>\r\n\
          To: <bob@golia.jp>\r\n\
          Subject: Important: Q4 numbers for review\r\n\
          Date: Sun, 22 May 2026 09:55:00 +0900\r\n\
          Message-ID: <abc-123@example.com>\r\n\
          DKIM-Signature: v=1; a=rsa-sha256; d=example.com; s=mail;\r\n\
              \tc=relaxed/relaxed; q=dns/txt; t=1716362100;\r\n\
              \th=From:To:Subject:Date:Message-ID;\r\n\
              \tbh=AbCdEf0123456789AbCdEf0123456789AbCdEf01234=;\r\n\
              \tb=DkimSignatureContentHere0123456789AbCdEf=\r\n\
          MIME-Version: 1.0\r\n\
          Content-Type: text/plain; charset=utf-8\r\n\
          Content-Transfer-Encoding: 7bit\r\n\r\n",
    );
    for _ in 0..(body_kb * 1024 / 80) {
        msg.extend_from_slice(
            b"This is a typical inbound message body line, ASCII text only.\r\n",
        );
    }
    msg
}

fn bench_header_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("header_lookup_subject_and_from");

    for &body_kb in &[1usize, 5, 20] {
        let msg = build_sample(body_kb);

        // mailrs-rfc5322: skip-ahead scan, stops at empty line
        group.bench_with_input(
            criterion::BenchmarkId::new("mailrs_rfc5322", body_kb),
            &msg,
            |b, msg| {
                b.iter(|| {
                    let m = Message::new(black_box(msg));
                    let s = m.header("Subject");
                    let f = m.header("From");
                    black_box((s, f))
                });
            },
        );

        // mail-parser: builds full Message tree
        group.bench_with_input(
            criterion::BenchmarkId::new("mail_parser", body_kb),
            &msg,
            |b, msg| {
                b.iter_batched(
                    || msg.clone(),
                    |msg| {
                        let parsed = mail_parser::MessageParser::default().parse(&msg);
                        let s = parsed.as_ref().and_then(|p| p.subject().map(|s| s.to_string()));
                        let f = parsed.as_ref().and_then(|p| {
                            p.from()
                                .and_then(|a| a.first())
                                .and_then(|addr| addr.address().map(|s| s.to_string()))
                        });
                        black_box((s, f))
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_body_offset(c: &mut Criterion) {
    let mut group = c.benchmark_group("body_offset_lookup");
    for &body_kb in &[1usize, 5, 20] {
        let msg = build_sample(body_kb);

        group.bench_with_input(
            criterion::BenchmarkId::new("mailrs_rfc5322", body_kb),
            &msg,
            |b, msg| {
                b.iter(|| {
                    let m = Message::new(black_box(msg));
                    black_box(m.body())
                });
            },
        );

        group.bench_with_input(
            criterion::BenchmarkId::new("mail_parser_body_text", body_kb),
            &msg,
            |b, msg| {
                b.iter_batched(
                    || msg.clone(),
                    |msg| {
                        let parsed = mail_parser::MessageParser::default().parse(&msg);
                        let body = parsed.as_ref().and_then(|p| p.body_text(0));
                        black_box(body.map(|s| s.len()))
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_received_chain(c: &mut Criterion) {
    let mut group = c.benchmark_group("received_chain_walk");
    let msg = build_sample(5);

    group.bench_function("mailrs_rfc5322", |b| {
        b.iter(|| {
            let m = Message::new(black_box(&msg));
            let mut count = 0;
            for _ in m.header_all("Received") {
                count += 1;
            }
            black_box(count)
        });
    });

    group.bench_function("mail_parser", |b| {
        b.iter_batched(
            || msg.clone(),
            |msg| {
                let parsed = mail_parser::MessageParser::default().parse(&msg);
                let count = parsed
                    .as_ref()
                    .map(|p| {
                        p.headers()
                            .iter()
                            .filter(|h| {
                                matches!(&h.name, mail_parser::HeaderName::Received)
                                    || matches!(&h.name, mail_parser::HeaderName::Other(n) if n.eq_ignore_ascii_case("received"))
                            })
                            .count()
                    })
                    .unwrap_or(0);
                black_box(count)
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

/// Worst-case header lookup: target is the LAST of 50 headers.
/// Mirrors realistic inbound shapes (SaaS notification mails with
/// many X-* metadata headers + Authentication-Results chains).
fn bench_header_lookup_target_at_end(c: &mut Criterion) {
    let mut raw = Vec::new();
    for i in 0..49 {
        raw.extend_from_slice(format!("X-Filler-{i}: value{i}\r\n").as_bytes());
    }
    raw.extend_from_slice(b"X-Target: bingo\r\n\r\nbody bytes here");

    c.bench_function("header_lookup_worst_case_50_headers", |b| {
        b.iter(|| {
            let m = Message::new(black_box(&raw));
            let r = m.header("X-Target");
            black_box(r)
        });
    });
}

/// Full iteration over a 50-header message via `headers()`.
fn bench_headers_iteration_50(c: &mut Criterion) {
    let mut raw = Vec::new();
    for i in 0..50 {
        raw.extend_from_slice(format!("X-Field-{i}: value{i}\r\n").as_bytes());
    }
    raw.extend_from_slice(b"\r\nbody");

    c.bench_function("headers_iter_50_fields", |b| {
        b.iter(|| {
            let m = Message::new(black_box(&raw));
            let count = m.headers().count();
            black_box(count)
        });
    });
}

/// `body_offset()` cached call — should be a Cell::get + branch.
/// Calls body() once to prime, then re-measures.
fn bench_body_offset_cached(c: &mut Criterion) {
    let msg = build_sample(5);
    let m = Message::new(&msg);
    let _ = m.body(); // prime the cache
    c.bench_function("body_offset_cached_call", |b| {
        b.iter(|| {
            let r = m.body_offset();
            black_box(r)
        });
    });
}

criterion_group!(
    benches,
    bench_header_lookup,
    bench_body_offset,
    bench_received_chain,
    bench_header_lookup_target_at_end,
    bench_headers_iteration_50,
    bench_body_offset_cached,
);
criterion_main!(benches);
