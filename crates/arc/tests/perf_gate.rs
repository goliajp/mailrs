//! Perf regression gates for the mailrs-arc parsers.
//!
//! Budgets are loose (~10× release P95) so this catches
//! order-of-magnitude regressions without flaking under load.

use std::time::{Duration, Instant};

use mailrs_arc::{ArcAuthResults, ArcChain, ArcMessageSignature, ArcSeal};

fn time<F: Fn()>(iterations: u32, f: F) -> Duration {
    // Warm up — JIT-equivalent for the page tables, the allocator,
    // and the criterion-style cache miss on first iter.
    for _ in 0..16 {
        f();
    }
    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    start.elapsed() / iterations
}

#[test]
fn aar_parse_under_budget() {
    let v = "i=1; spf=pass smtp.mailfrom=alice@example.com; dkim=pass header.d=example.com";
    let per = time(10_000, || {
        let _ = std::hint::black_box(ArcAuthResults::parse(std::hint::black_box(v)).unwrap());
    });
    assert!(
        per < Duration::from_micros(5),
        "ArcAuthResults::parse {per:?} (budget 5 µs)"
    );
}

#[test]
fn ams_parse_under_budget() {
    let v = "i=1; a=rsa-sha256; c=relaxed/relaxed; d=example.com; s=mail; \
             h=From:To:Subject:Date:Message-ID; bh=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=; \
             b=signature1234567890abcdefghijklmnopqrstuvwxyz";
    let per = time(10_000, || {
        let _ = std::hint::black_box(ArcMessageSignature::parse(std::hint::black_box(v)).unwrap());
    });
    assert!(
        per < Duration::from_micros(10),
        "ArcMessageSignature::parse {per:?} (budget 10 µs)"
    );
}

#[test]
fn as_parse_under_budget() {
    let v =
        "i=1; a=rsa-sha256; cv=none; d=example.com; s=mail; t=1700000000; b=SEAL1234567890abcdef";
    let per = time(10_000, || {
        let _ = std::hint::black_box(ArcSeal::parse(std::hint::black_box(v)).unwrap());
    });
    assert!(
        per < Duration::from_micros(10),
        "ArcSeal::parse {per:?} (budget 10 µs)"
    );
}

#[test]
fn chain_extract_two_hop_under_budget() {
    let msg = b"\
ARC-Authentication-Results: i=1; spf=pass\r\n\
ARC-Message-Signature: i=1; a=rsa-sha256; c=relaxed/relaxed; d=example.com; s=mail; h=From:To:Subject; bh=BH1; b=SIG1\r\n\
ARC-Seal: i=1; a=rsa-sha256; cv=none; d=example.com; s=mail; b=SEAL1\r\n\
ARC-Authentication-Results: i=2; dkim=pass\r\n\
ARC-Message-Signature: i=2; a=rsa-sha256; c=relaxed/relaxed; d=fwd.example; s=mail; h=From:To:Subject; bh=BH2; b=SIG2\r\n\
ARC-Seal: i=2; a=rsa-sha256; cv=pass; d=fwd.example; s=mail; b=SEAL2\r\n\
From: alice@example.com\r\n\r\nbody";
    let per = time(1_000, || {
        let _ = std::hint::black_box(ArcChain::extract(std::hint::black_box(msg)).unwrap());
    });
    assert!(
        per < Duration::from_micros(50),
        "ArcChain::extract two-hop {per:?} (budget 50 µs)"
    );
}
