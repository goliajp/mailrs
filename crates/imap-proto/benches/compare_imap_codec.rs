//! Head-to-head: `mailrs-imap-proto::parse_command` vs `imap-codec` 2.0.
//!
//! imap-codec is a nom-based "rock-solid and complete" IMAP codec
//! (alpha but widely-used). We benchmark the most common commands.

use criterion::{Criterion, criterion_group, criterion_main};
use imap_codec::CommandCodec;
use imap_codec::decode::Decoder;
use mailrs_imap_proto::parse_command;
use std::hint::black_box;

const SELECT: &[u8] = b"A001 SELECT INBOX\r\n";
const FETCH: &[u8] = b"A002 FETCH 1:100 (FLAGS BODY[HEADER.FIELDS (FROM SUBJECT DATE)])\r\n";
const LOGIN: &[u8] = b"A003 LOGIN alice@example.com password123\r\n";
const NOOP: &[u8] = b"A004 NOOP\r\n";

fn bench_select(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse/select");
    group.bench_function("mailrs_imap_proto", |b| {
        let s = std::str::from_utf8(SELECT)
            .unwrap()
            .trim_end_matches("\r\n");
        b.iter(|| black_box(parse_command(black_box(s)).unwrap()));
    });
    let codec = CommandCodec::default();
    group.bench_function("imap_codec", |b| {
        b.iter(|| black_box(codec.decode(black_box(SELECT)).ok()));
    });
    group.finish();
}

fn bench_fetch(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse/fetch");
    group.bench_function("mailrs_imap_proto", |b| {
        let s = std::str::from_utf8(FETCH).unwrap().trim_end_matches("\r\n");
        b.iter(|| black_box(parse_command(black_box(s)).unwrap()));
    });
    let codec = CommandCodec::default();
    group.bench_function("imap_codec", |b| {
        b.iter(|| black_box(codec.decode(black_box(FETCH)).ok()));
    });
    group.finish();
}

fn bench_login(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse/login");
    group.bench_function("mailrs_imap_proto", |b| {
        let s = std::str::from_utf8(LOGIN).unwrap().trim_end_matches("\r\n");
        b.iter(|| black_box(parse_command(black_box(s)).unwrap()));
    });
    let codec = CommandCodec::default();
    group.bench_function("imap_codec", |b| {
        b.iter(|| black_box(codec.decode(black_box(LOGIN)).ok()));
    });
    group.finish();
}

fn bench_noop(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse/noop");
    group.bench_function("mailrs_imap_proto", |b| {
        let s = std::str::from_utf8(NOOP).unwrap().trim_end_matches("\r\n");
        b.iter(|| black_box(parse_command(black_box(s)).unwrap()));
    });
    let codec = CommandCodec::default();
    group.bench_function("imap_codec", |b| {
        b.iter(|| black_box(codec.decode(black_box(NOOP)).ok()));
    });
    group.finish();
}

criterion_group!(benches, bench_select, bench_fetch, bench_login, bench_noop);
criterion_main!(benches);
