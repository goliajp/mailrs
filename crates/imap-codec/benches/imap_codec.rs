//! Criterion benches for `mailrs-imap-codec`. Covers both IMAP
//! framing modes plus the encoder:
//!
//! - **line mode** decode — CRLF-terminated commands and
//!   responses. Steady-state of every IMAP session.
//! - **literal mode** decode — byte-counted raw payloads from
//!   `expect_literal(N)`. The APPEND / FETCH BODY[…] hot path.
//! - **bare-CR skip** — RFC 9051 requires bare `\r` to be
//!   ignored (not used as framing). Exercises the memchr
//!   restart loop.
//! - **encode** — caller writes a `Vec<u8>` into a `BytesMut`
//!   (single `extend_from_slice` under the hood).

use bytes::BytesMut;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use mailrs_imap_codec::ImapCodec;
use std::hint::black_box;
use tokio_util::codec::{Decoder, Encoder};

fn bench_decode_line(c: &mut Criterion) {
    // Representative IMAP command lines, all CRLF-terminated.
    let frames: &[(&str, &[u8])] = &[
        ("noop", b"A001 NOOP\r\n"),
        ("login", b"a001 LOGIN user pass\r\n"),
        ("select", b"a002 SELECT INBOX\r\n"),
        // Long FETCH response with many sequence numbers + lots of
        // body. Approximates server-side `FETCH 1:100 BODY[…]`.
        (
            "fetch_long",
            b"* 1 FETCH (FLAGS (\\Seen) UID 1234 INTERNALDATE \"01-Jan-2024 12:00:00 +0000\" BODY[HEADER.FIELDS (FROM TO SUBJECT DATE MESSAGE-ID)] {180}\r\n",
        ),
    ];

    let mut group = c.benchmark_group("decode/line");
    for (name, frame) in frames {
        group.throughput(Throughput::Bytes(frame.len() as u64));
        group.bench_function(*name, |b| {
            b.iter(|| {
                let mut codec = ImapCodec::new();
                let mut buf = BytesMut::from(*frame);
                let r = codec.decode(black_box(&mut buf)).unwrap();
                black_box(r);
            });
        });
    }
    group.finish();
}

fn bench_decode_literal(c: &mut Criterion) {
    // Literal-mode decode at 3 payload sizes representing typical
    // IMAP body / APPEND shapes. Each iter rebuilds the codec and
    // buffer so the cost includes split_to + to_vec.
    for &n in &[32usize, 1_024, 102_400] {
        let mut payload = vec![b'x'; n];
        payload.extend_from_slice(b"\r\n");

        let mut group = c.benchmark_group("decode/literal");
        group.throughput(Throughput::Bytes(payload.len() as u64));
        group.bench_function(format!("{}b", n), |b| {
            b.iter(|| {
                let mut codec = ImapCodec::new();
                codec.expect_literal(n as u32);
                let mut buf = BytesMut::from(payload.as_slice());
                let r = codec.decode(black_box(&mut buf)).unwrap();
                black_box(r);
            });
        });
        group.finish();
    }
}

fn bench_decode_bare_cr_skip(c: &mut Criterion) {
    // Bare `\r` inside a line — memchr will hit it, the decoder
    // checks for `\n` following, finds none, advances past, and
    // resumes scanning. This exercises the restart loop in the
    // worst-realistic shape: a line with several embedded CRs
    // before the real CRLF terminator.
    let payload = b"hello\rworld\rfoo\rbar\rbaz\r\n";
    c.bench_function("decode/line/bare_cr_skip", |b| {
        b.iter(|| {
            let mut codec = ImapCodec::new();
            let mut buf = BytesMut::from(&payload[..]);
            let r = codec.decode(black_box(&mut buf)).unwrap();
            black_box(r);
        });
    });
}

fn bench_encode(c: &mut Criterion) {
    let short = b"* OK ready\r\n".to_vec();
    let long = b"* 1 FETCH (FLAGS (\\Seen) UID 1234 INTERNALDATE \"01-Jan-2024 12:00:00 +0000\" BODY[HEADER.FIELDS (FROM TO SUBJECT DATE)] {120}\r\n".to_vec();

    let mut group = c.benchmark_group("encode");
    group.bench_function("short_12b", |b| {
        b.iter(|| {
            let mut codec = ImapCodec::new();
            let mut dst = BytesMut::new();
            codec.encode(black_box(short.clone()), &mut dst).unwrap();
            black_box(dst);
        });
    });
    group.bench_function("long_140b", |b| {
        b.iter(|| {
            let mut codec = ImapCodec::new();
            let mut dst = BytesMut::new();
            codec.encode(black_box(long.clone()), &mut dst).unwrap();
            black_box(dst);
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_decode_line,
    bench_decode_literal,
    bench_decode_bare_cr_skip,
    bench_encode
);
criterion_main!(benches);
