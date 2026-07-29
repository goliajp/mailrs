//! Regression budgets for `mailrs-imap-format`. See BUDGETS.md.
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
