//! Regression budgets for `mailrs-smtp-codec`. See BUDGETS.md.

use mailrs_smtp_codec::has_smuggle_sequence;
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
fn has_smuggle_sequence_under_budget() {
    let payload = b"hello world\r\nthis is a typical DATA payload\r\n.\r\n";
    let median = time_median(|| {
        let _ = has_smuggle_sequence(payload);
    });
    // Budget: 10 µs (release < 100 ns for short payloads).
    assert!(
        median < Duration::from_micros(10),
        "has_smuggle_sequence median {median:?} exceeds 10µs"
    );
}
