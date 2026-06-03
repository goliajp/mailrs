use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use mailrs_imap_format::{
    extract_body_section, extract_header_section, find_line_offset, format_imap_flags,
    format_internal_date, parse_imap_flags,
};
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

/// Build a representative IMAP message: ~10 header lines + a body
/// of `body_kb` KB. Headers are ASCII (typical inbound shape); the
/// body is plain text. The CRLF CRLF separator sits at the
/// ~400-byte mark.
fn build_message(body_kb: usize) -> Vec<u8> {
    let mut msg = Vec::with_capacity(512 + body_kb * 1024);
    msg.extend_from_slice(
        b"Return-Path: <alice@example.com>\r\n\
          Received: from mta.example.com by mx.golia.jp; Sun, 22 May 2026 10:00:00 +0900\r\n\
          From: \"Alice\" <alice@example.com>\r\n\
          To: <bob@golia.jp>\r\n\
          Subject: Body section bench\r\n\
          Date: Sun, 22 May 2026 09:55:00 +0900\r\n\
          Message-ID: <abc-123@example.com>\r\n\
          MIME-Version: 1.0\r\n\
          Content-Type: text/plain; charset=utf-8\r\n\
          Content-Transfer-Encoding: 7bit\r\n\r\n",
    );
    for _ in 0..(body_kb * 1024 / 80) {
        msg.extend_from_slice(b"This is a typical inbound message body line, ASCII text only.\r\n");
    }
    msg
}

fn bench_extract_header(c: &mut Criterion) {
    // `extract_header_section` runs on every FETCH BODY[HEADER] /
    // FETCH BODY.PEEK[HEADER.FIELDS (...)]. Per-message hot path
    // when an IMAP client opens a mailbox view.
    let mut group = c.benchmark_group("extract_header_section");
    for &body_kb in &[1usize, 5, 20] {
        let msg = build_message(body_kb);
        group.throughput(Throughput::Bytes(msg.len() as u64));
        group.bench_function(format!("body_{}kb", body_kb), |b| {
            b.iter(|| {
                let r = extract_header_section(black_box(&msg));
                black_box(r);
            });
        });
    }
    group.finish();
}

fn bench_extract_body(c: &mut Criterion) {
    // `extract_body_section` runs on every FETCH BODY[TEXT] /
    // BODY[1] (single-part text body), per-message hot path.
    let mut group = c.benchmark_group("extract_body_section");
    for &body_kb in &[1usize, 5, 20] {
        let msg = build_message(body_kb);
        group.throughput(Throughput::Bytes(msg.len() as u64));
        group.bench_function(format!("body_{}kb", body_kb), |b| {
            b.iter(|| {
                let r = extract_body_section(black_box(&msg));
                black_box(r);
            });
        });
    }
    group.finish();
}

fn bench_find_line_offset(c: &mut Criterion) {
    // `find_line_offset` runs on every FETCH BODY[TEXT]<N.M>
    // partial fetch where the client asks for an octet range
    // starting from a logical line offset. Less common than the
    // section extractors but on the same per-FETCH path.
    let body = {
        let mut v = Vec::with_capacity(10_240);
        for _ in 0..130 {
            v.extend_from_slice(
                b"This is a typical inbound message body line, ASCII text only.\r\n",
            );
        }
        v
    };
    let mut group = c.benchmark_group("find_line_offset");
    for &line in &[1usize, 50, 120] {
        group.bench_function(format!("line_{}", line), |b| {
            b.iter(|| {
                let r = find_line_offset(black_box(&body), line);
                black_box(r);
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_format,
    bench_extract_header,
    bench_extract_body,
    bench_find_line_offset
);
criterion_main!(benches);
