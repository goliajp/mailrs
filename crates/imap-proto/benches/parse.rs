//! Micro-benchmarks for imap-proto hot paths.
//!
//! Run with: `cargo bench -p mailrs-imap-proto`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_imap_proto::command::parse_command;
use mailrs_imap_proto::response::{format_fetch, format_list};
use mailrs_imap_proto::sequence::{parse_sequence_set, sequence_set_to_uids};

fn bench_parse_command(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_command");
    group.bench_function("LOGIN", |b| {
        b.iter(|| parse_command(black_box("a001 LOGIN alice secret\r\n")))
    });
    group.bench_function("SELECT", |b| {
        b.iter(|| parse_command(black_box("a002 SELECT INBOX\r\n")))
    });
    group.bench_function("FETCH_uid_range", |b| {
        b.iter(|| parse_command(black_box("a003 FETCH 1:1000 (FLAGS BODY.PEEK[HEADER])\r\n")))
    });
    group.bench_function("UID_SEARCH_complex", |b| {
        b.iter(|| {
            parse_command(black_box(
                "a004 UID SEARCH SINCE 1-Jan-2026 NOT DELETED OR FROM alice@example.com SUBJECT urgent\r\n",
            ))
        })
    });
    group.finish();
}

fn bench_sequence_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequence_set");
    group.bench_function("parse_simple", |b| {
        b.iter(|| parse_sequence_set(black_box("1,3,5,7,9,11")))
    });
    group.bench_function("parse_ranges", |b| {
        b.iter(|| parse_sequence_set(black_box("1:100,200:300,400:500,*")))
    });

    let set = parse_sequence_set("1:1000,2000:3000,5000").unwrap();
    group.bench_function("expand_to_uids_n4001", |b| {
        b.iter(|| sequence_set_to_uids(black_box(&set), 10_000))
    });
    group.finish();
}

fn bench_format_response(c: &mut Criterion) {
    let mut group = c.benchmark_group("format_response");
    group.bench_function("list_typical", |b| {
        b.iter(|| {
            format_list(
                black_box("\\HasNoChildren"),
                black_box("/"),
                black_box("INBOX"),
            )
        })
    });
    let items = vec![
        ("FLAGS".to_string(), "(\\Seen \\Recent)".to_string()),
        (
            "INTERNALDATE".to_string(),
            "\"20-May-2026 12:00:00 +0900\"".to_string(),
        ),
        ("RFC822.SIZE".to_string(), "4096".to_string()),
        ("UID".to_string(), "42".to_string()),
    ];
    group.bench_function("fetch_4_items", |b| {
        b.iter(|| format_fetch(black_box(1), black_box(&items)))
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_parse_command,
    bench_sequence_set,
    bench_format_response
);
criterion_main!(benches);
