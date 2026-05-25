//! Regression budgets for `mailrs-attachment-extract`. See BUDGETS.md.

use std::time::{Duration, Instant};
use mailrs_attachment_extract::extraction_method;

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
fn extraction_method_dispatch_under_budget() {
    let median = time_median(|| {
        let _ = extraction_method("text/plain");
    });
    // Budget: 5 µs (release < 50 ns; dev ~500 ns). String prefix match.
    assert!(
        median < Duration::from_micros(5),
        "extraction_method median {median:?} exceeds 5µs"
    );
}
