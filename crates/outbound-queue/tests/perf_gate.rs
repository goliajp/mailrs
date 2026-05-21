//! Performance regression gates. See [BUDGETS.md](../BUDGETS.md).
//!
//! DKIM signing is intentionally NOT gated here — RSA signing in debug
//! mode is 50-100× slower than release, which would make a wall-clock
//! budget either useless or require running tests in `--release`. The
//! criterion bench in `benches/core.rs` covers that path; this file
//! enforces the cheap algorithmic paths only.

use std::time::{Duration, Instant};

use mailrs_outbound_queue::retry::{retry_delay_secs, should_bounce};

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

#[test]
fn retry_delay_full_sequence_under_budget() {
    let median = time_median(|| {
        for attempt in 0..10 {
            let _ = retry_delay_secs(attempt);
        }
    });
    // Budget: 10 µs. Observed P95: ~100 ns (pure arithmetic).
    let budget = Duration::from_micros(10);
    assert!(median < budget, "retry_delay_secs(0..10) median {median:?} exceeded {budget:?}");
}

#[test]
fn should_bounce_full_sequence_under_budget() {
    let median = time_median(|| {
        for attempt in 0..10 {
            let _ = should_bounce(attempt, 5);
        }
    });
    // Budget: 10 µs. Observed P95: ~50 ns.
    let budget = Duration::from_micros(10);
    assert!(median < budget, "should_bounce(0..10, 5) median {median:?} exceeded {budget:?}");
}
