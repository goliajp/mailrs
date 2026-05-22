//! Comparative bench: `mailrs-rfc2047::decode` vs `mail-parser`'s
//! `Subject` extraction (which also decodes encoded-words).

use criterion::{criterion_group, criterion_main, Criterion};
use mailrs_rfc2047::{decode, encode};
use std::hint::black_box;

fn bench_encode_ascii(c: &mut Criterion) {
    let input = "This is an ASCII subject, plain English with no encoding.";
    c.bench_function("encode/ascii_passthrough", |b| {
        b.iter(|| {
            let r = encode(black_box(input));
            black_box(r)
        });
    });
}

fn bench_encode_japanese(c: &mut Criterion) {
    let input = "日本語のメールサブジェクト";
    c.bench_function("encode/japanese", |b| {
        b.iter(|| {
            let r = encode(black_box(input));
            black_box(r)
        });
    });
}

fn bench_ascii_passthrough(c: &mut Criterion) {
    let input = b"This is an ASCII subject, plain English with no encoding.";
    c.bench_function("decode/ascii_passthrough", |b| {
        b.iter(|| {
            let r = decode(black_box(input));
            black_box(r)
        });
    });
}

fn bench_utf8_b_simple(c: &mut Criterion) {
    let input = b"=?UTF-8?B?VGVzdCBNZXNzYWdl?=";
    c.bench_function("decode/utf8_B_simple", |b| {
        b.iter(|| {
            let r = decode(black_box(input));
            black_box(r)
        });
    });
}

fn bench_utf8_q_simple(c: &mut Criterion) {
    let input = b"=?UTF-8?Q?Hello=20World=20!?=";
    c.bench_function("decode/utf8_Q_simple", |b| {
        b.iter(|| {
            let r = decode(black_box(input));
            black_box(r)
        });
    });
}

fn bench_iso_2022_jp(c: &mut Criterion) {
    let input = b"=?ISO-2022-JP?B?GyRCJDMkcyRLJEEkTxsoQg==?=";
    c.bench_function("decode/iso_2022_jp", |b| {
        b.iter(|| {
            let r = decode(black_box(input));
            black_box(r)
        });
    });
}

fn bench_mixed_with_ascii(c: &mut Criterion) {
    let input = b"Re: =?UTF-8?B?VGVzdA==?= regarding the proposal";
    c.bench_function("decode/mixed_ascii_and_encoded", |b| {
        b.iter(|| {
            let r = decode(black_box(input));
            black_box(r)
        });
    });
}

fn bench_vs_mail_parser_subject_ascii(c: &mut Criterion) {
    // Compare: getting Subject from a full message via mail-parser
    // (which does encoded-word decoding internally) vs
    // mailrs-rfc5322::header + mailrs-rfc2047::decode.
    let msg = b"\
Subject: This is an ASCII subject\r\n\
From: alice@example.com\r\n\
\r\n\
body\r\n";

    let mut group = c.benchmark_group("subject_extraction_ascii");
    group.bench_function("mail_parser_full_parse", |b| {
        b.iter(|| {
            let p = mail_parser::MessageParser::default().parse(black_box(msg));
            let s = p.as_ref().and_then(|m| m.subject().map(|s| s.to_string()));
            black_box(s)
        });
    });
    group.bench_function("rfc2047_only_for_subject", |b| {
        // Hand-roll the equivalent: get the Subject bytes (this skips
        // the full RFC 5322 parser; we just use std byte ops here as
        // a proxy for what mailrs-rfc5322 would do — actual rfc5322
        // bench is in its own crate).
        b.iter(|| {
            let bytes = black_box(msg);
            // Naive scan for "Subject:" (just for the bench framing).
            let idx = bytes
                .windows(8)
                .position(|w| w.eq_ignore_ascii_case(b"Subject:"))
                .unwrap();
            let line_end = bytes[idx..]
                .iter()
                .position(|&b| b == b'\n')
                .unwrap();
            let value = &bytes[idx + 8..idx + line_end];
            let trimmed = if !value.is_empty() && value[0] == b' ' {
                &value[1..]
            } else {
                value
            };
            let r = decode(trimmed);
            black_box(r)
        });
    });
    group.finish();
}

fn bench_vs_mail_parser_subject_encoded(c: &mut Criterion) {
    let msg = b"\
Subject: =?UTF-8?B?VGVzdCBNZXNzYWdlIFN1YmplY3Q=?=\r\n\
From: alice@example.com\r\n\
\r\n\
body\r\n";

    let mut group = c.benchmark_group("subject_extraction_encoded");
    group.bench_function("mail_parser_full_parse", |b| {
        b.iter(|| {
            let p = mail_parser::MessageParser::default().parse(black_box(msg));
            let s = p.as_ref().and_then(|m| m.subject().map(|s| s.to_string()));
            black_box(s)
        });
    });
    group.bench_function("rfc2047_only_for_subject", |b| {
        b.iter(|| {
            let bytes = black_box(msg);
            let idx = bytes
                .windows(8)
                .position(|w| w.eq_ignore_ascii_case(b"Subject:"))
                .unwrap();
            let line_end = bytes[idx..]
                .iter()
                .position(|&b| b == b'\n')
                .unwrap();
            let value = &bytes[idx + 8..idx + line_end];
            let trimmed = if !value.is_empty() && value[0] == b' ' {
                &value[1..]
            } else {
                value
            };
            let r = decode(trimmed);
            black_box(r)
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_ascii_passthrough,
    bench_utf8_b_simple,
    bench_utf8_q_simple,
    bench_iso_2022_jp,
    bench_mixed_with_ascii,
    bench_vs_mail_parser_subject_ascii,
    bench_vs_mail_parser_subject_encoded,
    bench_encode_ascii,
    bench_encode_japanese,
);
criterion_main!(benches);
