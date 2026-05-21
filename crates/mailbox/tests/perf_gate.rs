//! Performance regression gates for mailbox's pure-algorithm helpers.
//!
//! PG-bound ops are not gated here — their cost is dominated by network /
//! DB latency, not in-process logic. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_mailbox::threading::{
    extract_in_reply_to, extract_message_id, normalize_message_id, resolve_thread_id,
};

const ITERS: usize = 100;

fn time_median<F: FnMut()>(mut op: F) -> Duration {
    let mut samples = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let start = Instant::now();
        op();
        samples.push(start.elapsed());
    }
    samples.sort();
    samples[ITERS / 2]
}

const LONG_HEADER_MESSAGE: &[u8] = b"\
Return-Path: <bounce@aol.com>\r\n\
Received: from mta-out-1.aol.com by spam-check.example.com\r\n\
\twith ESMTPS id 1234567890ABC; Wed, 20 May 2026 11:50:00 +0900\r\n\
DKIM-Signature: v=1; a=rsa-sha256; c=relaxed/relaxed; d=aol.com; s=20221111;\r\n\
\tt=1715000000; bh=AAAAAAAAAAAAAAAAAAAAA=; h=From:To:Subject:Date;\r\n\
\tb=BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB\r\n\
From: \"Alice Reynolds\" <alice@example.com>\r\n\
To: bob@example.com, carol@example.com, dave@example.com\r\n\
Subject: =?UTF-8?B?VGVzdCDmtYvor5U=?=\r\n\
Date: Wed, 20 May 2026 12:00:00 +0900\r\n\
Message-ID: <xxxxxx-yyyyyy-zzzz@mailer.example.com>\r\n\
References: <root1@x> <root2@x> <middle1@x> <middle2@x> <prev@x>\r\n\
In-Reply-To: <prev@x>\r\n\
\r\n\
body\r\n";

#[test]
fn extract_message_id_long_headers_under_budget() {
    let median = time_median(|| {
        let _ = extract_message_id(LONG_HEADER_MESSAGE);
    });
    // Budget: 100 µs. Observed P95: ~3 µs.
    let budget = Duration::from_micros(100);
    assert!(median < budget, "extract_message_id median {median:?} exceeded {budget:?}");
}

#[test]
fn extract_in_reply_to_long_headers_under_budget() {
    let median = time_median(|| {
        let _ = extract_in_reply_to(LONG_HEADER_MESSAGE);
    });
    // Budget: 100 µs. Observed P95: ~3 µs.
    let budget = Duration::from_micros(100);
    assert!(median < budget, "extract_in_reply_to median {median:?} exceeded {budget:?}");
}

#[test]
fn normalize_message_id_under_budget() {
    let median = time_median(|| {
        let _ = normalize_message_id("  <abc-123@example.com>  ");
    });
    // Budget: 20 µs. Observed P95: ~200 ns.
    let budget = Duration::from_micros(20);
    assert!(median < budget, "normalize_message_id median {median:?} exceeded {budget:?}");
}

#[test]
fn resolve_thread_id_known_parent_under_budget() {
    let median = time_median(|| {
        let _ = resolve_thread_id("<msg2@x>", "<msg1@x>", |_: &str| {
            Some("thread-abc".to_string())
        });
    });
    // Budget: 50 µs. Observed P95: ~500 ns.
    let budget = Duration::from_micros(50);
    assert!(median < budget, "resolve_thread_id median {median:?} exceeded {budget:?}");
}
