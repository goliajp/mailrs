//! Regression budgets for `mailrs-dkim`. See [BUDGETS.md](../BUDGETS.md).
//!
//! Release-profile only. Every budget in this file was measured against
//! an optimised build (`canon_body/relaxed` is ~140 ns there against a
//! 5 µs budget); a dev build runs the same code roughly 35× slower, so
//! the margin that looks like 35× is actually about zero. It held only
//! while the machine was idle — under the parallel load `cargo test
//! --workspace` creates for itself the median crossed 5 µs and the gate
//! went red, ten out of ten times green when run alone.
//!
//! A gate that reports how busy the host is, rather than how fast the
//! code is, teaches everyone to skip past red. Rather than inflate the
//! numbers — which would drop real coverage in release too — the file
//! compiles only when the budgets are meaningful. Run it with
//! `cargo test -p mailrs-dkim --release`.
//!
//! Same treatment as `crates/mailrs-mmalloc/tests/perf_gate.rs`.
#![cfg(not(debug_assertions))]

use std::time::{Duration, Instant};

use mailrs_dkim::canon::{canonicalize_body, canonicalize_header};
use mailrs_dkim::header::{Canon, DkimHeader};

const ITERS: usize = 200;

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

#[test]
fn parse_minimal_under_budget() {
    let header = "v=1; a=rsa-sha256; d=e.com; s=s; h=From; bh=AAAA; b=BBBB";
    let median = time_median(|| {
        let _ = DkimHeader::parse(header);
    });
    // Budget: 10 µs (release ~700 ns; dev mode ~5× slower for tag-list
    // hash map building).
    let budget = Duration::from_micros(30);
    assert!(median < budget, "parse_minimal {median:?} > {budget:?}");
}

#[test]
fn parse_realistic_under_budget() {
    let header = " v=1; a=rsa-sha256; c=relaxed/relaxed; d=example.com; s=mail;\r\n\
                   \th=From:To:Subject:Date:Message-ID:MIME-Version:Content-Type;\r\n\
                   \tbh=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=;\r\n\
                   \tb=signature1234567890abcdefghijklmnopqrstuvwxyz";
    let median = time_median(|| {
        let _ = DkimHeader::parse(header);
    });
    // Budget: 20 µs (release ~1.5 µs).
    let budget = Duration::from_micros(20);
    assert!(median < budget, "parse_realistic {median:?} > {budget:?}");
}

#[test]
fn canonicalize_body_simple_under_budget() {
    let body = b"Hello world.\r\n\r\nThis is a test message.\r\n\r\n\r\n";
    let median = time_median(|| {
        let _ = canonicalize_body(body, Canon::Simple, None);
    });
    // Budget: 5 µs (release ~70 ns).
    let budget = Duration::from_micros(5);
    assert!(median < budget, "canon_body/simple {median:?} > {budget:?}");
}

#[test]
fn canonicalize_body_relaxed_under_budget() {
    let body = b"Hello   world.  \r\nLine\twith\ttabs   \r\n\r\n\r\n";
    let median = time_median(|| {
        let _ = canonicalize_body(body, Canon::Relaxed, None);
    });
    // Budget: 5 µs (release ~140 ns).
    let budget = Duration::from_micros(5);
    assert!(
        median < budget,
        "canon_body/relaxed {median:?} > {budget:?}"
    );
}

#[test]
fn canonicalize_header_relaxed_under_budget() {
    let median = time_median(|| {
        let _ = canonicalize_header("From", " alice@example.com", Canon::Relaxed);
    });
    // Budget: 5 µs (release ~85 ns).
    let budget = Duration::from_micros(5);
    assert!(
        median < budget,
        "canon_header/relaxed {median:?} > {budget:?}"
    );
}
