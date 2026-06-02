//! Criterion benches for `mailrs-smtp-codec`. Covers the three
//! published hot paths:
//!
//! - `SmtpCodec::decode` in command and data modes — the actual
//!   prod entry point: a Tokio Decoder pulled per SMTP frame
//! - `has_smuggle_sequence` — bare-LF dot-terminator scan, used
//!   by Strict mode on every DATA payload
//! - `normalize_line_endings` — CRLF normaliser, used by
//!   Permissive mode (default) on every DATA payload
//!
//! Each helper is benched on a small input (matches the
//! micro-baseline) and several payload-size buckets that represent
//! real DATA-mode mail bodies (1 KB / 10 KB / 100 KB).

use bytes::BytesMut;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use mailrs_smtp_codec::{
    SmtpCodec, SmuggleProtection, has_smuggle_sequence, normalize_line_endings,
};
use std::hint::black_box;
use tokio_util::codec::Decoder;

fn bench_smuggle(c: &mut Criterion) {
    let safe = b"hello\r\n.\r\n";
    c.bench_function("has_smuggle_sequence/safe", |b| {
        b.iter(|| {
            let _ = has_smuggle_sequence(black_box(safe));
        });
    });

    for &n in &[1_024usize, 10_240, 102_400] {
        let mut payload = vec![b'a'; n];
        payload.extend_from_slice(b"\r\n.\r\n");
        let mut group = c.benchmark_group("has_smuggle_sequence");
        group.throughput(Throughput::Bytes(payload.len() as u64));
        group.bench_function(format!("clean_{}b", n), |b| {
            b.iter(|| {
                let _ = has_smuggle_sequence(black_box(&payload));
            });
        });
        group.finish();
    }
}

fn bench_normalize(c: &mut Criterion) {
    let lf_only = b"hello\nworld\n";
    c.bench_function("normalize_line_endings/lf_only", |b| {
        b.iter(|| {
            let _ = normalize_line_endings(black_box(lf_only));
        });
    });

    // Realistic body shapes: ~80-char lines terminated by bare LF
    // (worst case for the normaliser — every line ending rewrites).
    for &n in &[1_024usize, 10_240, 102_400] {
        let mut payload = Vec::with_capacity(n);
        while payload.len() < n {
            payload.extend_from_slice(&[b'x'; 79]);
            payload.push(b'\n');
        }
        payload.truncate(n);
        let mut group = c.benchmark_group("normalize_line_endings");
        group.throughput(Throughput::Bytes(payload.len() as u64));
        group.bench_function(format!("bare_lf_{}b", n), |b| {
            b.iter(|| {
                let _ = normalize_line_endings(black_box(&payload));
            });
        });
        group.finish();
    }
}

fn bench_decode_command(c: &mut Criterion) {
    // Tokio Decoder consumes a BytesMut; each decode call pops one
    // CRLF-terminated frame. Bench the steady-state shape: feed one
    // command-line at a time.
    let frames: &[&[u8]] = &[
        b"EHLO mail.example.com\r\n",
        b"MAIL FROM:<alice@example.com> SIZE=10240\r\n",
        b"RCPT TO:<bob@example.com>\r\n",
        b"DATA\r\n",
    ];
    let names = ["ehlo", "mail_from", "rcpt_to", "data"];

    let mut group = c.benchmark_group("decode/command");
    for (frame, name) in frames.iter().zip(names) {
        group.throughput(Throughput::Bytes(frame.len() as u64));
        group.bench_function(name, |b| {
            b.iter(|| {
                let mut codec = SmtpCodec::new();
                let mut buf = BytesMut::from(*frame);
                let r = codec.decode(black_box(&mut buf)).unwrap();
                black_box(r);
            });
        });
    }
    group.finish();
}

fn bench_decode_data(c: &mut Criterion) {
    // Data-mode decode — the per-message hot path. Build a payload
    // of N body bytes followed by the CRLF.CRLF terminator and time
    // a full decode. Test each smuggle-protection mode.
    let modes = [
        (SmuggleProtection::Permissive, "permissive"),
        (SmuggleProtection::Strict, "strict"),
        (SmuggleProtection::Off, "off"),
    ];

    for &n in &[1_024usize, 10_240, 102_400] {
        let mut payload = Vec::with_capacity(n + 5);
        while payload.len() < n {
            payload.extend_from_slice(b"line content here, ascii only\r\n");
        }
        payload.truncate(n);
        payload.extend_from_slice(b"\r\n.\r\n");

        for (mode, mode_name) in modes {
            let mut group = c.benchmark_group("decode/data");
            group.throughput(Throughput::Bytes(payload.len() as u64));
            group.bench_function(format!("{}_{}b", mode_name, n), |b| {
                b.iter(|| {
                    let mut codec = SmtpCodec::new().with_smuggle_protection(mode);
                    codec.enter_data_mode();
                    let mut buf = BytesMut::from(payload.as_slice());
                    let r = codec.decode(black_box(&mut buf)).unwrap();
                    black_box(r);
                });
            });
            group.finish();
        }
    }
}

criterion_group!(
    benches,
    bench_smuggle,
    bench_normalize,
    bench_decode_command,
    bench_decode_data
);
criterion_main!(benches);
