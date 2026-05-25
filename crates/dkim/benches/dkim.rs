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

criterion_group!(
    benches,
    bench_parse_minimal,
    bench_parse_realistic,
    bench_canonicalize_body_simple,
    bench_canonicalize_body_relaxed,
    bench_canonicalize_header_relaxed,
);
criterion_main!(benches);
