//! mail-builder perf bench — v4 ckpt 24.
//!
//! Two scan paths got rewritten to memchr in this ckpt:
//!   * `strict::find_header_terminator` — `windows(4)` walking for
//!     `\r\n\r\n` body separator in `lint()`.
//!   * `multipart::contains_subslice` — `windows().any()` boundary
//!     collision scan in `generate_boundary()`.
//!
//! Beyond those, the typical outbound path is `MessageBuilder::build`
//! (DSN / bounce / postmaster mail) and `build_strict` (same +
//! lint pass that uses the new memchr scan).

use criterion::{Criterion, criterion_group, criterion_main};
use mailrs_mail_builder::{Attachment, MessageBuilder, PartBytes, lint, multipart_envelope};
use std::hint::black_box;

fn build_short_plain() -> Vec<u8> {
    MessageBuilder::new()
        .from("postmaster@golia.jp")
        .to("user@example.com")
        .subject("delivery failure")
        .text_body("Mail for x failed: 550 5.1.1 User unknown.\r\n")
        .build()
}

fn build_plain_with_html() -> Vec<u8> {
    MessageBuilder::new()
        .from("alice@golia.jp")
        .to("bob@example.com")
        .subject("hello multipart/alternative")
        .text_body("plain version here\r\n".repeat(20))
        .html_body("<p>html version here</p>\r\n".repeat(20))
        .build()
}

fn build_with_attachment() -> Vec<u8> {
    let blob: Vec<u8> = (0..16 * 1024).map(|i| (i & 0xff) as u8).collect();
    MessageBuilder::new()
        .from("alice@golia.jp")
        .to("bob@example.com")
        .subject("with 16 KiB attachment")
        .text_body("see attached\r\n")
        .attachment(Attachment::new(
            "data.bin",
            "application/octet-stream",
            blob,
        ))
        .build()
}

fn bench_build_paths(c: &mut Criterion) {
    let mut group = c.benchmark_group("mail_builder/build");
    group.bench_function("short_plain", |b| {
        b.iter(|| {
            let m = MessageBuilder::new()
                .from(black_box("postmaster@golia.jp"))
                .to(black_box("user@example.com"))
                .subject(black_box("delivery failure"))
                .text_body(black_box("Mail for x failed: 550 5.1.1 User unknown.\r\n"));
            black_box(m.build());
        });
    });
    group.bench_function("plain_plus_html", |b| {
        b.iter(|| {
            let m = MessageBuilder::new()
                .from(black_box("alice@golia.jp"))
                .to(black_box("bob@example.com"))
                .subject(black_box("hello multipart/alternative"))
                .text_body(black_box("plain version here\r\n".repeat(20)))
                .html_body(black_box("<p>html version here</p>\r\n".repeat(20)));
            black_box(m.build());
        });
    });
    group.bench_function("with_16k_attachment", |b| {
        let blob: Vec<u8> = (0..16 * 1024).map(|i| (i & 0xff) as u8).collect();
        b.iter(|| {
            let m = MessageBuilder::new()
                .from(black_box("alice@golia.jp"))
                .to(black_box("bob@example.com"))
                .subject(black_box("with 16 KiB attachment"))
                .text_body(black_box("see attached\r\n"))
                .attachment(Attachment::new(
                    "data.bin",
                    "application/octet-stream",
                    blob.clone(),
                ));
            black_box(m.build());
        });
    });
    group.finish();
}

fn bench_lint(c: &mut Criterion) {
    let short = build_short_plain();
    let mixed = build_plain_with_html();
    let big = build_with_attachment();
    let mut group = c.benchmark_group("mail_builder/lint");
    group.bench_function("short_plain", |b| {
        b.iter(|| {
            let _ = lint(black_box(&short));
        });
    });
    group.bench_function("plain_plus_html", |b| {
        b.iter(|| {
            let _ = lint(black_box(&mixed));
        });
    });
    group.bench_function("with_16k_attachment", |b| {
        b.iter(|| {
            let _ = lint(black_box(&big));
        });
    });
    group.finish();
}

fn bench_envelope(c: &mut Criterion) {
    // multipart_envelope runs the boundary-collision scan on every
    // part — that's the memmem rewrite. Two-part alt + 3-part w/
    // 16k attachment cover the realistic outbound shapes.
    let alt_parts = vec![
        PartBytes {
            headers:
                b"Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 7bit\r\n"
                    .to_vec(),
            body: b"plain version of the message body\r\n".repeat(20),
        },
        PartBytes {
            headers:
                b"Content-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: 7bit\r\n"
                    .to_vec(),
            body: b"<p>html version of the message body</p>\r\n".repeat(20),
        },
    ];
    let mixed_parts = vec![
        PartBytes {
            headers: b"Content-Type: text/plain\r\n".to_vec(),
            body: b"see attached\r\n".to_vec(),
        },
        PartBytes {
            headers: b"Content-Type: application/octet-stream\r\nContent-Transfer-Encoding: base64\r\nContent-Disposition: attachment; filename=data.bin\r\n".to_vec(),
            body: (0..16 * 1024).map(|i| ((i & 0xff) as u8 % 64) + b'A').collect(),
        },
    ];
    let mut group = c.benchmark_group("mail_builder/envelope");
    group.bench_function("alternative_small", |b| {
        b.iter(|| {
            black_box(multipart_envelope(black_box(&alt_parts)));
        });
    });
    group.bench_function("mixed_with_16k_attachment", |b| {
        b.iter(|| {
            black_box(multipart_envelope(black_box(&mixed_parts)));
        });
    });
    group.finish();
}

criterion_group!(benches, bench_build_paths, bench_lint, bench_envelope);
criterion_main!(benches);
