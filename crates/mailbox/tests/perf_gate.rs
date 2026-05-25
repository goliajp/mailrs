//! Performance regression gates for mailbox's pure-algorithm helpers.
//!
//! PG-bound ops are not gated here — their cost is dominated by network /
//! DB latency, not in-process logic. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_mailbox::fixtures::{EXAMPLE_USER, InMemoryMailboxStore};
use mailrs_mailbox::threading::{
    extract_in_reply_to, extract_message_id, normalize_message_id, resolve_thread_id,
};
use mailrs_mailbox::{
    FLAG_ANSWERED, FLAG_DELETED, FLAG_DRAFT, FLAG_FLAGGED, FLAG_SEEN, InsertMessage, MailboxStore,
    QueryFilter, bitmask_to_maildir_flags, maildir_flags_to_bitmask,
};
use mailrs_maildir::Flag;

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
    assert!(
        median < budget,
        "extract_message_id median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn extract_in_reply_to_long_headers_under_budget() {
    let median = time_median(|| {
        let _ = extract_in_reply_to(LONG_HEADER_MESSAGE);
    });
    // Budget: 100 µs. Observed P95: ~3 µs.
    let budget = Duration::from_micros(100);
    assert!(
        median < budget,
        "extract_in_reply_to median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn normalize_message_id_under_budget() {
    let median = time_median(|| {
        let _ = normalize_message_id("  <abc-123@example.com>  ");
    });
    // Budget: 20 µs. Observed P95: ~200 ns.
    let budget = Duration::from_micros(20);
    assert!(
        median < budget,
        "normalize_message_id median {median:?} exceeded {budget:?}"
    );
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
    assert!(
        median < budget,
        "resolve_thread_id median {median:?} exceeded {budget:?}"
    );
}

// ===== Flag bitmask conversion gates =====
//
// Both directions run on every message touched by IMAP FETCH FLAGS,
// IMAP STORE, JMAP keyword query, and every maildir round-trip on delivery.
// Per-message cost — pure bit-twiddling + a tiny Vec alloc — so the budget
// is intentionally very tight; any future change that adds I/O, locking,
// or large allocations will blow well past these.

#[test]
fn maildir_flags_to_bitmask_batch_under_budget() {
    // Single conversion is below the timer floor (<100 ns), so we batch
    // 100 calls per sample to get a measurable median. A typical IMAP
    // FETCH returns 50-200 messages, each requiring one of these
    // conversions — this batch matches a real per-FETCH burst.
    let flags = vec![Flag::Seen, Flag::Replied, Flag::Flagged];
    let median = time_median(|| {
        for _ in 0..100 {
            let _ = maildir_flags_to_bitmask(&flags);
        }
    });
    // Budget: 120 µs (~20× headroom). Observed P95 (dev): ~6 µs for 100
    // calls (≈ 60 ns per call). Pure bit-OR over a small slice; an
    // order-of-magnitude regression — e.g. accidental allocation per call,
    // an O(n^2) variant, or any introduced syscall — will exceed this.
    let budget = Duration::from_micros(120);
    assert!(
        median < budget,
        "maildir_flags_to_bitmask x100 median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn bitmask_to_maildir_flags_batch_under_budget() {
    // Batched (100×) for the same reason as the forward direction: per-call
    // cost (~40 ns) sits below the timer floor. The function allocates a
    // `Vec<Flag>` of up to 5 elements per call, so this batch is also the
    // most-allocations-per-FETCH path in the conversion pair.
    let bits = FLAG_SEEN | FLAG_ANSWERED | FLAG_FLAGGED | FLAG_DELETED | FLAG_DRAFT;
    let median = time_median(|| {
        for _ in 0..100 {
            let _ = bitmask_to_maildir_flags(bits);
        }
    });
    // Budget: 200 µs (~22× headroom). Observed P95 (dev): ~9 µs for 100
    // calls (≈ 90 ns each — pushes through 100 small Vec allocations).
    // Any future change that switches the return type to something heavier
    // or adds I/O will trip this gate.
    let budget = Duration::from_micros(200);
    assert!(
        median < budget,
        "bitmask_to_maildir_flags x100 median {median:?} exceeded {budget:?}"
    );
}

// ===== InsertMessage clone overhead =====
//
// `MailboxStore::insert_message` takes `InsertMessage<'_>` by value, so any
// caller that retains the input has to clone it. The struct is borrow-based
// (13 fields, mostly `&str`) — `Clone` is field-by-field reference copy
// plus a few integers. Hot path: every inbound delivery clones at least
// once when audit/log/event-bus paths fan out. We batch 100 clones per
// sample because one clone is below the timer floor.
#[test]
fn insert_message_clone_batch_under_budget() {
    let input = InsertMessage {
        user: "alice@example.com",
        mailbox_name: "INBOX",
        blob_ref: "1715000000.M123P456.host:2,",
        sender: "Alice <alice@example.com>",
        recipients: "bob@example.com, carol@example.com",
        subject: "Re: project status update",
        size: 12_345,
        date: 1_715_000_000,
        internal_date: 1_715_000_100,
        message_id: "abc-123-def@host",
        in_reply_to: "prev-msg@host",
        thread_id: "thread-xyz",
        flags: FLAG_SEEN | FLAG_FLAGGED,
    };
    let median = time_median(|| {
        for _ in 0..100 {
            let _ = std::hint::black_box(&input).clone();
        }
    });
    // Budget: 30 µs (~30× headroom). Observed P95 (dev): ~1 µs for 100
    // clones (≈ 10 ns each). The struct is all `&str` + plain integers —
    // `Clone` is essentially a memcpy of the borrow descriptors. If this
    // ever exceeds the budget it means someone added an owning field on
    // the hot path (e.g. switched a `&str` to `String`). `black_box` on
    // the input prevents the compiler from eliding the clone entirely.
    let budget = Duration::from_micros(30);
    assert!(
        median < budget,
        "InsertMessage::clone x100 median {median:?} exceeded {budget:?}"
    );
}

// ===== QueryFilter predicate against 200 messages =====
//
// Runs the InMemoryMailboxStore `query_messages` against a 200-message
// mailbox with a text-substring + has_keyword filter. Mirrors a typical
// JMAP `Email/query` shape. This gate covers the predicate cost (lowercase
// + contains across three fields per message) plus the sort + paginate
// tail. Useful for catching regressions where someone adds an `O(n^2)`
// filter or an expensive per-message allocation inside the matcher.
#[tokio::test]
async fn query_filter_200_messages_under_budget() {
    let store = InMemoryMailboxStore::new();
    let mbox = store.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();

    // Seed: 200 messages with varied subjects; half have FLAG_SEEN, half don't.
    let _mbox_id = mbox.id;
    for i in 0..200u32 {
        let subj = if i % 7 == 0 {
            format!("Project alpha update #{i}")
        } else if i % 5 == 0 {
            format!("Re: meeting agenda {i}")
        } else {
            format!("Newsletter issue {i}")
        };
        let mid = format!("msg-{i}@host");
        let flags = if i % 2 == 0 { FLAG_SEEN } else { 0 };
        let input = InsertMessage {
            user: EXAMPLE_USER,
            mailbox_name: "INBOX",
            blob_ref: "blob",
            sender: "sender@example.com",
            recipients: "alice@example.com",
            subject: &subj,
            size: 1024,
            date: 1_715_000_000 + i64::from(i),
            internal_date: 1_715_000_000 + i64::from(i),
            message_id: &mid,
            in_reply_to: "",
            thread_id: &mid,
            flags,
        };
        store.insert_message(input).await.unwrap();
    }

    // Measure: query for "project" in subject AND has FLAG_SEEN set.
    let mut samples = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let filter = QueryFilter {
            user: Some(EXAMPLE_USER),
            text: Some("project"),
            has_keyword: Some(FLAG_SEEN),
            limit: 50,
            ..Default::default()
        };
        let start = Instant::now();
        let _ = store.query_messages(filter).await.unwrap();
        samples.push(start.elapsed());
    }
    samples.sort();
    let median = samples[ITERS / 2];

    // Budget: 1.5 ms (~20× headroom). Observed P95 (dev): ~70 µs. Per-msg
    // cost is dominated by `to_lowercase()` of sender/recipients/subject
    // followed by three `contains()` checks. 200 messages × ~350 ns ≈
    // 70 µs. Order-of-magnitude regressions (e.g. someone replaces the
    // linear scan with O(n^2) or adds a sync I/O call per match) will trip
    // this gate well before they hit production.
    let budget = Duration::from_micros(1_500);
    assert!(
        median < budget,
        "query_messages(200 msgs) median {median:?} exceeded {budget:?}"
    );
}
