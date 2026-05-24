use criterion::{criterion_group, criterion_main, Criterion};
use mailrs_arf::parse;
use std::hint::black_box;

const HOTMAIL_FBL_SAMPLE: &[u8] = b"From: staff@hotmail.com\r\n\
Subject: complaint about message from bulk@example.com\r\n\
Content-Type: multipart/report; report-type=feedback-report;\r\n\
\tboundary=\"----=BOUNDARY\"\r\n\
\r\n\
------=BOUNDARY\r\n\
Content-Type: message/feedback-report\r\n\
\r\n\
Feedback-Type: abuse\r\n\
User-Agent: Hotmail FBL\r\n\
Version: 1\r\n\
Original-Mail-From: <bulk@example.com>\r\n\
Original-Rcpt-To: <victim@hotmail.com>\r\n\
Arrival-Date: Sun, 25 May 2026 10:00:00 +0000\r\n\
Source-IP: 192.0.2.42\r\n\
Reported-Domain: example.com\r\n\
Authentication-Results: hotmail.com;\r\n\
\tspf=pass smtp.mailfrom=bulk@example.com;\r\n\
\tdkim=pass header.d=example.com\r\n\
\r\n\
------=BOUNDARY--\r\n";

const NOT_ARF_SAMPLE: &[u8] = b"From: alice@example.com\r\n\
To: bob@example.com\r\n\
Subject: lunch?\r\n\
\r\n\
Want to grab lunch tomorrow?\r\n";

fn bench_parse_hotmail(c: &mut Criterion) {
    c.bench_function("parse/hotmail_fbl_sample", |b| {
        b.iter(|| {
            let r = parse(black_box(HOTMAIL_FBL_SAMPLE));
            black_box(r)
        });
    });
}

fn bench_parse_not_arf(c: &mut Criterion) {
    c.bench_function("parse/not_arf_early_exit", |b| {
        b.iter(|| {
            let r = parse(black_box(NOT_ARF_SAMPLE));
            black_box(r)
        });
    });
}

criterion_group!(benches, bench_parse_hotmail, bench_parse_not_arf);
criterion_main!(benches);
