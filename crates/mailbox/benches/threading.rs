//! Micro-benchmarks for the pure-algorithm helpers in mailrs-mailbox.
//!
//! Run with: `cargo bench -p mailrs-mailbox`.
//!
//! PG-bound ops are not benched here — their cost is dominated by network
//! and DB latency, not the in-process logic.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_mailbox::threading::{
    extract_in_reply_to, extract_message_id, normalize_message_id, resolve_thread_id,
};

const SHORT_MESSAGE: &[u8] = b"\
From: alice@example.com\r\n\
To: bob@example.com\r\n\
Subject: Re: hello\r\n\
Date: Wed, 20 May 2026 12:00:00 +0900\r\n\
Message-ID: <abc123@example.com>\r\n\
In-Reply-To: <prev42@example.com>\r\n\
References: <root@example.com> <prev42@example.com>\r\n\
\r\n\
Hello!\r\n";

const LONG_HEADER_MESSAGE: &[u8] = b"\
Return-Path: <bounce@aol.com>\r\n\
Received: from mta-out-1.aol.com by spam-check.example.com\r\n\
\twith ESMTPS id 1234567890ABC; Wed, 20 May 2026 11:50:00 +0900\r\n\
Authentication-Results: spam-check.example.com; spf=pass smtp.mailfrom=aol.com\r\n\
DKIM-Signature: v=1; a=rsa-sha256; c=relaxed/relaxed; d=aol.com;\r\n\
\ts=20221111; t=1715000000; bh=AAAAAAAAAAAAAAAAAAAAA=;\r\n\
\th=From:To:Subject:Date;\r\n\
\tb=BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB\r\n\
From: \"Alice Reynolds\" <alice@example.com>\r\n\
To: bob@example.com, carol@example.com, dave@example.com\r\n\
Cc: eve@example.com\r\n\
Subject: =?UTF-8?B?VGVzdCDmtYvor5U=?=\r\n\
Date: Wed, 20 May 2026 12:00:00 +0900\r\n\
Message-ID: <xxxxxx-yyyyyy-zzzz@mailer.example.com>\r\n\
References: <root1@example.com> <root2@example.com> <middle1@example.com> <middle2@example.com> <prev@example.com>\r\n\
In-Reply-To: <prev@example.com>\r\n\
List-Unsubscribe: <mailto:unsub@aol.com>, <https://aol.com/unsub>\r\n\
List-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/alternative; boundary=\"==boundary42==\"\r\n\
X-Mailer: AOL Mail webclient 7.0\r\n\
\r\n\
--==boundary42==\r\n\
Content-Type: text/plain; charset=utf-8\r\n\r\nbody\r\n";

fn bench_extract_headers(c: &mut Criterion) {
    let mut group = c.benchmark_group("extract_headers");
    group.bench_function("message_id_short", |b| {
        b.iter(|| extract_message_id(black_box(SHORT_MESSAGE)))
    });
    group.bench_function("message_id_long_headers", |b| {
        b.iter(|| extract_message_id(black_box(LONG_HEADER_MESSAGE)))
    });
    group.bench_function("in_reply_to_short", |b| {
        b.iter(|| extract_in_reply_to(black_box(SHORT_MESSAGE)))
    });
    group.bench_function("in_reply_to_long_headers", |b| {
        b.iter(|| extract_in_reply_to(black_box(LONG_HEADER_MESSAGE)))
    });
    group.finish();
}

fn bench_normalize(c: &mut Criterion) {
    c.bench_function("normalize_message_id", |b| {
        b.iter(|| normalize_message_id(black_box("  <abc-123@example.com>  ")))
    });
}

fn bench_resolve_thread(c: &mut Criterion) {
    let mut group = c.benchmark_group("resolve_thread_id");
    group.bench_function("new_root", |b| {
        b.iter(|| {
            resolve_thread_id(black_box("<msg1@x>"), black_box(""), |_: &str| None)
        })
    });
    group.bench_function("known_parent", |b| {
        b.iter(|| {
            resolve_thread_id(
                black_box("<msg2@x>"),
                black_box("<msg1@x>"),
                |_: &str| Some("thread-abc".to_string()),
            )
        })
    });
    group.finish();
}

criterion_group!(benches, bench_extract_headers, bench_normalize, bench_resolve_thread);
criterion_main!(benches);
