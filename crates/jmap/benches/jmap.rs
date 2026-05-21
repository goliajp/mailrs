//! Microbenchmarks for the pure helpers + composition paths (no live store
//! hits).
//!
//! For dispatcher / envelope benches that DO hit a store (an in-memory one),
//! see `benches/dispatch.rs`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_jmap::build::{
    build_email_meta, epoch_to_utc_string, extend_with_body, parse_address_list, wants_body,
};
use mailrs_jmap::flags::{flags_to_keywords, keywords_to_flags};
use mailrs_jmap::ids::{parse_email_db_id, parse_mailbox_db_id};
use mailrs_jmap::refs::resolve_references;
use mailrs_jmap::types::{
    Attachment, FLAG_ANSWERED, FLAG_FLAGGED, FLAG_SEEN, Message, ParsedBody,
};

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

fn sample_message() -> Message {
    Message {
        id: 1,
        mailbox_id: 2,
        uid: 3,
        sender: "Alice <alice@example.com>".into(),
        recipients: "Bob <bob@example.com>, carol@example.com".into(),
        subject: "weekly digest".into(),
        date: 1_700_000_000,
        size: 4096,
        flags: FLAG_SEEN | FLAG_ANSWERED,
        internal_date: 1_700_000_001,
        message_id: "abc123@example.com".into(),
        in_reply_to: "parent@example.com".into(),
        thread_id: "thread-42".into(),
        user_address: "alice@example.com".into(),
        new_content: Some("preview snippet text here".into()),
        blob_id: "blob-1".into(),
    }
}

fn sample_parsed_body() -> ParsedBody {
    ParsedBody {
        text: Some("hello\nworld\n".into()),
        html: Some("<p>hello</p>".into()),
        attachments: vec![Attachment {
            filename: "report.pdf".into(),
            content_type: "application/pdf".into(),
            size: 102_400,
        }],
    }
}

fn bench_meta_composition(c: &mut Criterion) {
    let msg = sample_message();
    let parsed = sample_parsed_body();

    // include-all path (selector = None) — what most clients send
    c.bench_function("build_email_meta_include_all", |b| {
        b.iter(|| build_email_meta(black_box(&msg), black_box("msg-1"), black_box(&None)))
    });

    // narrow selector — common pagination view (just subject + from)
    let narrow: Option<Vec<&str>> = Some(vec!["subject", "from"]);
    c.bench_function("build_email_meta_narrow_selector", |b| {
        b.iter(|| build_email_meta(black_box(&msg), black_box("msg-1"), black_box(&narrow)))
    });

    // include-all + body — the Email/get with body path
    c.bench_function("extend_with_body_full", |b| {
        b.iter(|| {
            let mut obj = build_email_meta(&msg, "msg-1", &None);
            extend_with_body(
                black_box(&mut obj),
                black_box(Some(&parsed)),
                black_box(&None),
            );
            obj
        })
    });

    // body fields requested but none available (raw was missing) — common
    // degraded path
    c.bench_function("extend_with_body_empty", |b| {
        b.iter(|| {
            let mut obj = build_email_meta(&msg, "msg-1", &None);
            extend_with_body(black_box(&mut obj), black_box(None), black_box(&None));
            obj
        })
    });

    // wants_body branching — called once per Email/get message
    let body_selector: Option<Vec<&str>> = Some(vec!["bodyValues", "textBody"]);
    let meta_selector: Option<Vec<&str>> = Some(vec!["subject", "from", "receivedAt"]);
    c.bench_function("wants_body_yes", |b| {
        b.iter(|| wants_body(black_box(&body_selector)))
    });
    c.bench_function("wants_body_no", |b| {
        b.iter(|| wants_body(black_box(&meta_selector)))
    });
}

criterion_group!(
    benches,
    bench_flags,
    bench_ids,
    bench_build,
    bench_refs,
    bench_meta_composition
);
criterion_main!(benches);
