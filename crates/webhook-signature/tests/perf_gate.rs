//! Regression budgets. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_webhook_signature::{format_header, parse_header, sign, verify};

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
fn sign_short_under_budget() {
    let secret = b"32-byte-shared-webhook-secret-aa";
    let payload = b"{\"event\":\"new_message\"}";
    let median = time_median(|| {
        let _ = sign(secret, payload);
    });
    // Budget: 30 µs (dev ~8 µs, release ~250 ns; HMAC ~25× slower in dev).
    let budget = Duration::from_micros(30);
    assert!(median < budget, "sign_short median {median:?} > {budget:?}");
}

#[test]
fn verify_correct_under_budget() {
    let secret = b"32-byte-shared-webhook-secret-aa";
    let payload = b"{\"event\":\"new_message\"}";
    let sig = sign(secret, payload);
    let median = time_median(|| {
        let _ = verify(secret, payload, &sig);
    });
    // Budget: 30 µs (release ~280 ns).
    let budget = Duration::from_micros(30);
    assert!(
        median < budget,
        "verify_correct median {median:?} > {budget:?}"
    );
}

#[test]
fn format_header_under_budget() {
    let sig = "a".repeat(64);
    let median = time_median(|| {
        let _ = format_header(&sig);
    });
    // Budget: 5 µs (release ~10 ns).
    let budget = Duration::from_micros(5);
    assert!(
        median < budget,
        "format_header median {median:?} > {budget:?}"
    );
}

#[test]
fn parse_header_under_budget() {
    let value = "sha256=abcdef0123456789";
    let median = time_median(|| {
        let _ = parse_header(value);
    });
    // Budget: 5 µs (release ~3 ns).
    let budget = Duration::from_micros(5);
    assert!(
        median < budget,
        "parse_header median {median:?} > {budget:?}"
    );
}
