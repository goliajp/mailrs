//! Regression budgets. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_backoff::{Backoff, Jitter};

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
fn base_delay_under_budget() {
    let b = Backoff::smtp_outbound();
    let median = time_median(|| {
        let _ = b.base_delay(3);
    });
    // Budget: 1 µs (release ~1.2 ns). Pure float math.
    let budget = Duration::from_micros(1);
    assert!(median < budget, "base_delay median {median:?} > {budget:?}");
}

#[test]
fn delay_none_under_budget() {
    let b = Backoff {
        initial: Duration::from_secs(60),
        multiplier: 2.0,
        max: Duration::from_secs(3600),
        jitter: Jitter::None,
    };
    let median = time_median(|| {
        let _ = b.delay(3, 42);
    });
    let budget = Duration::from_micros(1);
    assert!(median < budget, "delay/none median {median:?} > {budget:?}");
}

#[test]
fn delay_full_jitter_under_budget() {
    let b = Backoff::smtp_outbound();
    let median = time_median(|| {
        let _ = b.delay(3, 42);
    });
    // Budget: 1 µs (release ~3 ns). SplitMix64 + modulo.
    let budget = Duration::from_micros(1);
    assert!(median < budget, "delay/full median {median:?} > {budget:?}");
}

#[test]
fn delay_high_attempt_capped_under_budget() {
    let b = Backoff::smtp_outbound();
    // attempt=100 — multiplier^100 is huge but cap saves us
    let median = time_median(|| {
        let _ = b.delay(100, 42);
    });
    // Budget: 1 µs. multiplier^100 + min + jitter.
    let budget = Duration::from_micros(1);
    assert!(median < budget, "delay/high_attempt median {median:?} > {budget:?}");
}
