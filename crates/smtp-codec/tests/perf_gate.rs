//! Regression budgets for `mailrs-smtp-codec`. See BUDGETS.md.

use mailrs_smtp_codec::{has_smuggle_sequence, normalize_line_endings};
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

#[test]
fn normalize_line_endings_under_budget() {
    // 1 KB body with bare-LF line endings every 80 bytes — represents
    // the worst case (every line ending rewrites). Per v4 round 1
    // measurements, release ≈ 152 ns. Budget at 100 µs gives ~660×
    // headroom: catches order-of-magnitude regressions (e.g.
    // accidental removal of the memchr2-anchored chunked copy).
    let mut payload = Vec::with_capacity(1024);
    while payload.len() < 1024 {
        payload.extend_from_slice(&[b'x'; 79]);
        payload.push(b'\n');
    }
    payload.truncate(1024);
    let median = time_median(|| {
        let _ = normalize_line_endings(&payload);
    });
    assert!(
        median < Duration::from_micros(100),
        "normalize_line_endings median {median:?} exceeds 100µs"
    );
}
