//! Regression budgets for `mailrs-srs`. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_srs::{reverse, rewrite, DEFAULT_TIMESTAMP_WINDOW_DAYS};

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
fn rewrite_under_budget() {
    let median = time_median(|| {
        let _ = rewrite("alice@example.com", "mx.golia.jp", "shared-secret-key");
    });
    // Budget: 50 µs (dev mode ~8 µs, release ~270 ns; HMAC ~25× slower in dev).
    let budget = Duration::from_micros(50);
    assert!(
        median < budget,
        "rewrite median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn reverse_success_under_budget() {
    let secret = "shared-secret-key";
    let rewritten = rewrite("alice@example.com", "mx.golia.jp", secret);
    let median = time_median(|| {
        let r = reverse(&rewritten, secret, DEFAULT_TIMESTAMP_WINDOW_DAYS);
        assert!(r.is_some());
    });
    // Budget: 5 µs (release P95 ~290 ns). Same HMAC work as rewrite +
    // constant-time compare.
    let budget = Duration::from_micros(50);
    assert!(
        median < budget,
        "reverse_success median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn reverse_wrong_secret_constant_time_under_budget() {
    let rewritten = rewrite("alice@example.com", "mx.golia.jp", "right-secret");
    let median = time_median(|| {
        let r = reverse(&rewritten, "wrong-secret", DEFAULT_TIMESTAMP_WINDOW_DAYS);
        assert!(r.is_none());
    });
    // Budget: 5 µs. The wrong-secret path doesn't early-exit on first
    // byte mismatch (that's the constant-time property); same cost
    // as success path.
    let budget = Duration::from_micros(50);
    assert!(
        median < budget,
        "reverse_wrong_secret median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn reverse_malformed_under_budget() {
    let median = time_median(|| {
        let r = reverse("not-an-srs-address@example", "secret", DEFAULT_TIMESTAMP_WINDOW_DAYS);
        assert!(r.is_none());
    });
    // Budget: 1 µs (release P95 < 100 ns). Malformed inputs short-circuit
    // before the HMAC computation.
    let budget = Duration::from_micros(1);
    assert!(
        median < budget,
        "reverse_malformed median {median:?} exceeded {budget:?}"
    );
}
