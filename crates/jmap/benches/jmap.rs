//! Microbenchmarks for the pure helpers (no live store hits).

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_jmap::build::{epoch_to_utc_string, parse_address_list};
use mailrs_jmap::flags::{flags_to_keywords, keywords_to_flags};
use mailrs_jmap::ids::{parse_email_db_id, parse_mailbox_db_id};
use mailrs_jmap::refs::resolve_references;
use mailrs_jmap::types::{FLAG_ANSWERED, FLAG_FLAGGED, FLAG_SEEN};

fn bench_flags(c: &mut Criterion) {
    c.bench_function("flags_to_keywords", |b| {
        b.iter(|| flags_to_keywords(black_box(FLAG_SEEN | FLAG_ANSWERED | FLAG_FLAGGED)))
    });
    let kw = serde_json::json!({"$seen": true, "$answered": true, "$flagged": true});
    c.bench_function("keywords_to_flags", |b| {
        b.iter(|| keywords_to_flags(black_box(&kw)))
    });
}

fn bench_ids(c: &mut Criterion) {
    c.bench_function("parse_email_db_id", |b| {
        b.iter(|| parse_email_db_id(black_box("msg-123456")))
    });
    c.bench_function("parse_mailbox_db_id", |b| {
        b.iter(|| parse_mailbox_db_id(black_box("mb-42")))
    });
}

fn bench_build(c: &mut Criterion) {
    c.bench_function("parse_address_list", |b| {
        b.iter(|| {
            parse_address_list(black_box(
                "Alice <alice@example.com>, bob@example.com, Carol <carol@example.com>",
            ))
        })
    });
    c.bench_function("epoch_to_utc_string", |b| {
        b.iter(|| epoch_to_utc_string(black_box(1_700_000_000)))
    });
}

fn bench_refs(c: &mut Criterion) {
    let previous = vec![(
        "Email/query".to_string(),
        serde_json::json!({"ids": ["msg-1", "msg-2", "msg-3"]}),
        "c1".to_string(),
    )];
    c.bench_function("resolve_references", |b| {
        b.iter(|| {
            let mut args = serde_json::json!({
                "#ids": {"resultOf": "c1", "name": "Email/query", "path": "/ids"}
            });
            resolve_references(&mut args, black_box(&previous));
        })
    });
}

criterion_group!(benches, bench_flags, bench_ids, bench_build, bench_refs);
criterion_main!(benches);
