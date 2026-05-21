//! Performance regression gates. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_postmaster::extract_bimi_logo_url;

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
fn extract_bimi_logo_url_under_budget() {
    let record = "v=BIMI1; l=https://example.com/logo.svg; a=https://example.com/cert.pem";
    let median = time_median(|| {
        let _ = extract_bimi_logo_url(record);
    });
    // Budget: 20 µs. Observed P95: ~200 ns.
    let budget = Duration::from_micros(20);
    assert!(median < budget, "extract_bimi_logo_url median {median:?} exceeded {budget:?}");
}
