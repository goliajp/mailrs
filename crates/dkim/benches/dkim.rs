use criterion::{Criterion, criterion_group, criterion_main};
use mailrs_dkim::header::DkimHeader;
use std::hint::black_box;

fn bench_parse_minimal(c: &mut Criterion) {
    let header = "v=1; a=rsa-sha256; d=e.com; s=s; h=From; bh=AAAA; b=BBBB";
    c.bench_function("parse/minimal", |b| {
        b.iter(|| {
            let r = DkimHeader::parse(black_box(header));
            black_box(r.unwrap())
        });
    });
}

fn bench_parse_realistic(c: &mut Criterion) {
    let header = " v=1; a=rsa-sha256; c=relaxed/relaxed; d=example.com; s=mail;\r\n\
                   \th=From:To:Subject:Date:Message-ID:MIME-Version:Content-Type;\r\n\
                   \tbh=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=;\r\n\
                   \tb=signature1234567890abcdefghijklmnopqrstuvwxyz";
    c.bench_function("parse/realistic", |b| {
        b.iter(|| {
            let r = DkimHeader::parse(black_box(header));
            black_box(r.unwrap())
        });
    });
}

fn bench_canonicalize_body_simple(c: &mut Criterion) {
    use mailrs_dkim::canon::canonicalize_body;
    use mailrs_dkim::header::Canon;
    let body = b"Hello world.\r\n\r\nThis is a test message.\r\n\r\n\r\n";
    c.bench_function("canon_body/simple", |b| {
        b.iter(|| {
            let r = canonicalize_body(black_box(body), Canon::Simple, None);
            black_box(r)
        });
    });
}

fn bench_canonicalize_body_relaxed(c: &mut Criterion) {
    use mailrs_dkim::canon::canonicalize_body;
    use mailrs_dkim::header::Canon;
    let body = b"Hello   world.  \r\nLine\twith\ttabs   \r\n\r\n\r\n";
    c.bench_function("canon_body/relaxed", |b| {
        b.iter(|| {
            let r = canonicalize_body(black_box(body), Canon::Relaxed, None);
            black_box(r)
        });
    });

    // Larger-input shapes: every outbound DKIM sign runs body canon
    // over the full message body. The win above (40 B input) is
    // dominated by Vec setup; this is the realistic shape.
    let mut group = c.benchmark_group("canon_body/relaxed");
    for &kb in &[1usize, 5, 50] {
        let mut payload = Vec::with_capacity(kb * 1024);
        while payload.len() < kb * 1024 {
            payload.extend_from_slice(b"This is  a body  line\twith\tsome    interior wsp  \r\n");
        }
        payload.truncate(kb * 1024);
        group.throughput(criterion::Throughput::Bytes(payload.len() as u64));
        group.bench_function(format!("{}kb", kb), |b| {
            b.iter(|| {
                let r = canonicalize_body(black_box(&payload), Canon::Relaxed, None);
                black_box(r);
            });
        });
    }
    group.finish();
}

fn bench_canonicalize_header_relaxed(c: &mut Criterion) {
    use mailrs_dkim::canon::canonicalize_header;
    use mailrs_dkim::header::Canon;
    c.bench_function("canon_header/relaxed", |b| {
        b.iter(|| {
            let r = canonicalize_header(
                black_box("From"),
                black_box(" alice@example.com"),
                Canon::Relaxed,
            );
            black_box(r)
        });
    });
}

/// Build a header block with N realistic inbound headers — Return-Path,
/// Received, From, To, Subject, Date, Message-ID, MIME-Version,
/// Content-Type, then N-9 more `X-Filler-N:` lines. Used to bench the
/// per-DKIM-sign walk in `collect_signed_headers` and the per-verify
/// `find_header_value_in_raw` lookup.
fn build_header_block(n_filler: usize) -> Vec<u8> {
    let mut h = Vec::with_capacity(512 + n_filler * 32);
    h.extend_from_slice(
        b"Return-Path: <alice@example.com>\r\n\
          Received: from mta.example.com by mx.golia.jp;\r\n\
          \tSun, 22 May 2026 10:00:00 +0900\r\n\
          From: \"Alice\" <alice@example.com>\r\n\
          To: <bob@golia.jp>\r\n\
          Subject: DKIM bench message\r\n\
          Date: Sun, 22 May 2026 09:55:00 +0900\r\n\
          Message-ID: <abc-123@example.com>\r\n\
          MIME-Version: 1.0\r\n\
          Content-Type: text/plain; charset=utf-8\r\n",
    );
    for i in 0..n_filler {
        let line = format!("X-Filler-{i}: value{i} for filler header\r\n");
        h.extend_from_slice(line.as_bytes());
    }
    h
}

fn bench_collect_signed_headers(c: &mut Criterion) {
    use mailrs_dkim::headers::{collect_signed_headers, collect_signed_headers_borrowed};
    // Standard DKIM `h=` list — the canonical 5 sign-mandatory + a
    // few extras the canonical mailrs config asks for.
    let names: Vec<String> = ["From", "To", "Subject", "Date", "Message-ID"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut group = c.benchmark_group("collect_signed_headers");
    for &n_filler in &[0usize, 20, 50] {
        let headers = build_header_block(n_filler);
        // Owned variant (back-compat wrapper)
        group.bench_function(format!("owned/n_headers_{}", 10 + n_filler), |b| {
            b.iter(|| {
                let r = collect_signed_headers(black_box(&headers), black_box(&names));
                black_box(r);
            });
        });
        // Borrowed variant (zero-alloc, internal hot path)
        group.bench_function(format!("borrowed/n_headers_{}", 10 + n_filler), |b| {
            b.iter(|| {
                let r = collect_signed_headers_borrowed(black_box(&headers), black_box(&names));
                black_box(r);
            });
        });
    }
    group.finish();
}

fn bench_find_header_value(c: &mut Criterion) {
    use mailrs_dkim::headers::find_header_value;
    // First-header hit (Return-Path) vs deep-into-block hit
    // (Content-Type, last "real" header before the X-Fillers).
    let headers = build_header_block(50);
    let mut group = c.benchmark_group("find_header_value");
    group.bench_function("first_return_path", |b| {
        b.iter(|| {
            let r = find_header_value(black_box(&headers), "Return-Path");
            black_box(r);
        });
    });
    group.bench_function("mid_content_type", |b| {
        b.iter(|| {
            let r = find_header_value(black_box(&headers), "Content-Type");
            black_box(r);
        });
    });
    group.bench_function("missing", |b| {
        b.iter(|| {
            let r = find_header_value(black_box(&headers), "X-Does-Not-Exist");
            black_box(r);
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_parse_minimal,
    bench_parse_realistic,
    bench_canonicalize_body_simple,
    bench_canonicalize_body_relaxed,
    bench_canonicalize_header_relaxed,
    bench_collect_signed_headers,
    bench_find_header_value,
);
criterion_main!(benches);
