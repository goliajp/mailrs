//! Regression budgets for `mailrs-imap-format`. See BUDGETS.md.

use mailrs_imap_format::{format_imap_flags, parse_imap_flags};
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
fn format_flags_under_budget() {
    let median = time_median(|| {
        let _ = format_imap_flags(0b1111);
    });
    // Budget: 10 µs (release < 100 ns).
    assert!(
        median < Duration::from_micros(10),
        "format_imap_flags median {median:?} exceeds 10µs"
    );
}

#[test]
fn parse_flags_under_budget() {
    let median = time_median(|| {
        let _ = parse_imap_flags("\\Seen \\Answered \\Flagged");
    });
    assert!(
        median < Duration::from_micros(10),
        "parse_imap_flags median {median:?} exceeds 10µs"
    );
}
