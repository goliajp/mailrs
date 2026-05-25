use criterion::{Criterion, criterion_group, criterion_main};
use mail_parser::MimeHeaders;
use mailrs_mime::parse;
use std::hint::black_box;

const SIMPLE: &[u8] = b"Content-Type: text/plain\r\n\r\nhello world";

const MULTIPART: &[u8] = b"Content-Type: multipart/alternative; boundary=\"x\"\r\n\
\r\n\
--x\r\n\
Content-Type: text/plain\r\n\
\r\n\
plain text body\r\n\
--x\r\n\
Content-Type: text/html\r\n\
\r\n\
<p>html body</p>\r\n\
--x--\r\n";

const INVITE: &[u8] = b"Content-Type: multipart/alternative; boundary=\"x\"\r\n\
\r\n\
--x\r\n\
Content-Type: text/plain\r\n\
\r\n\
Meeting invitation\r\n\
--x\r\n\
Content-Type: text/calendar; method=REQUEST; charset=utf-8\r\n\
\r\n\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n\
--x--\r\n";

fn bench_parse_simple(c: &mut Criterion) {
    c.bench_function("parse/simple_text_plain", |b| {
        b.iter(|| {
            let p = parse(black_box(SIMPLE));
            black_box(p)
        });
    });
}

fn bench_parse_multipart(c: &mut Criterion) {
    c.bench_function("parse/multipart_alternative", |b| {
        b.iter(|| {
            let p = parse(black_box(MULTIPART));
            black_box(p)
        });
    });
}

fn bench_find_text_calendar(c: &mut Criterion) {
    c.bench_function("find_by_content_type/text_calendar", |b| {
        b.iter(|| {
            let p = parse(black_box(INVITE));
            let cal = p.find_by_content_type("text/calendar");
            black_box(cal.map(|x| x.body.len()))
        });
    });
}

fn bench_vs_mail_parser_simple(c: &mut Criterion) {
    let mut group = c.benchmark_group("vs_mail_parser/simple");
    group.bench_function("mailrs_mime", |b| {
        b.iter(|| {
            let p = parse(black_box(SIMPLE));
            black_box(p.body_text())
        });
    });
    group.bench_function("mail_parser", |b| {
        b.iter(|| {
            let parsed = mail_parser::MessageParser::default().parse(black_box(SIMPLE));
            black_box(parsed.and_then(|p| p.body_text(0).map(|s| s.into_owned())))
        });
    });
    group.finish();
}

fn bench_vs_mail_parser_invite(c: &mut Criterion) {
    // Fair comparison: "find the text/calendar part and return its body length".
    // Both libraries walk the MIME tree; we measure the same end-to-end work.
    let mut group = c.benchmark_group("vs_mail_parser/find_calendar");
    group.bench_function("mailrs_mime", |b| {
        b.iter(|| {
            let p = parse(black_box(INVITE));
            black_box(
                p.find_by_content_type("text/calendar")
                    .map(|x| x.body.len()),
            )
        });
    });
    group.bench_function("mail_parser", |b| {
        b.iter(|| {
            let parsed = mail_parser::MessageParser::default().parse(black_box(INVITE));
            // Apples-to-apples: walk parts, find the one whose content-type
            // is `text/calendar`, return its decoded body length. This is the
            // actual same operation `mailrs_mime::Part::find_by_content_type`
            // does for the same input.
            let r = parsed.and_then(|m| {
                // mail-parser ships an `is_content_type(type, subtype)` helper
                // that walks each part's content_type header — same logical
                // operation as our `find_by_content_type("text/calendar")`.
                m.parts.iter().find_map(|p| {
                    if p.is_content_type("text", "calendar") {
                        Some(p.contents().len())
                    } else {
                        None
                    }
                })
            });
            black_box(r)
        });
    });
    group.finish();
}

// Base64 decode benchmark — exercises the WSP-fast-path the
// v2.0.1 decoder added. `clean` is a single-line payload (no WSP);
// `wrapped` is RFC 2045 76-col line-wrapped.
fn bench_decode_base64(c: &mut Criterion) {
    use base64::Engine as _;

    let payload = vec![0x5Au8; 4096];
    let encoded_clean = base64::engine::general_purpose::STANDARD.encode(&payload);
    let mut encoded_wrapped = String::new();
    for (i, ch) in encoded_clean.chars().enumerate() {
        encoded_wrapped.push(ch);
        if (i + 1) % 76 == 0 {
            encoded_wrapped.push_str("\r\n");
        }
    }

    // Build a full multipart message whose only body is base64 4 KiB.
    let mk = |body_b64: &str| {
        format!(
            "Content-Type: application/octet-stream\r\n\
             Content-Transfer-Encoding: base64\r\n\r\n{body_b64}"
        )
        .into_bytes()
    };
    let clean = mk(&encoded_clean);
    let wrapped = mk(&encoded_wrapped);

    let mut group = c.benchmark_group("decode_base64");
    group.bench_function("clean_4k", |b| {
        b.iter(|| {
            let p = parse(black_box(&clean));
            black_box(p.body.len())
        });
    });
    group.bench_function("wrapped_4k", |b| {
        b.iter(|| {
            let p = parse(black_box(&wrapped));
            black_box(p.body.len())
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_parse_simple,
    bench_parse_multipart,
    bench_find_text_calendar,
    bench_vs_mail_parser_simple,
    bench_vs_mail_parser_invite,
    bench_decode_base64,
);
criterion_main!(benches);
