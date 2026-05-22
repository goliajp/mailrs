//! Regression budgets for `mailrs-clamav`. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_clamav::parse_response;

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
fn parse_clean_under_budget() {
    let median = time_median(|| {
        let _ = parse_response(b"stream: OK\n");
    });
    // Budget: 2 µs (release ~25 ns).
    let budget = Duration::from_micros(2);
    assert!(median < budget, "parse_clean median {median:?} > {budget:?}");
}

#[test]
fn parse_virus_under_budget() {
    let median = time_median(|| {
        let _ = parse_response(b"stream: Eicar-Test-Signature FOUND\n");
    });
    // Budget: 5 µs (release ~70 ns).
    let budget = Duration::from_micros(5);
    assert!(median < budget, "parse_virus median {median:?} > {budget:?}");
}

#[test]
fn parse_error_under_budget() {
    let median = time_median(|| {
        let _ = parse_response(b"INSTREAM size limit exceeded. ERROR");
    });
    // Budget: 5 µs (release ~95 ns).
    let budget = Duration::from_micros(5);
    assert!(median < budget, "parse_error median {median:?} > {budget:?}");
}
