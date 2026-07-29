//! Regression budgets for `mailrs-attachment-extract`. See BUDGETS.md.
//!
//! Release-profile only. The budgets here were derived from an optimised
//! build. A dev build runs the same code roughly an order of magnitude
//! slower, so in debug they assert how contended the host is rather than
//! how fast the code is — `cargo test --workspace` runs hundreds of test
//! binaries at once, and under that burst dkim and mime each went red
//! while passing ten out of ten in isolation.
//!
//! Nothing here is weakened: the numbers are untouched and still enforced
//! where they were measured. Run them with `./scripts/perf-gates.sh`.
#![cfg(not(debug_assertions))]

use mailrs_attachment_extract::extraction_method;
use std::time::{Duration, Instant};

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
