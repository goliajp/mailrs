//! Performance regression gates. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_maildir::{Flag, add_flag, parse_flags, serialize_flags};

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
fn parse_flags_all_standard_under_budget() {
    let median = time_median(|| {
        let _ = parse_flags("FRPST");
    });
    // Budget: 5 µs. Observed P95: ~50 ns.
    let budget = Duration::from_micros(5);
    assert!(median < budget, "parse_flags(FRPST) median {median:?} exceeded {budget:?}");
}

#[test]
fn serialize_flags_all_standard_under_budget() {
    let all = vec![Flag::Flagged, Flag::Replied, Flag::Passed, Flag::Seen, Flag::Trashed];
    let median = time_median(|| {
        let _ = serialize_flags(&all);
    });
    // Budget: 5 µs. Observed P95: ~50 ns.
    let budget = Duration::from_micros(5);
    assert!(median < budget, "serialize_flags(5 flags) median {median:?} exceeded {budget:?}");
}

#[test]
fn add_flag_to_existing_under_budget() {
    let median = time_median(|| {
        let _ = add_flag("FR", Flag::Seen);
    });
    // Budget: 5 µs. Observed P95: ~50 ns.
    let budget = Duration::from_micros(5);
    assert!(median < budget, "add_flag median {median:?} exceeded {budget:?}");
}
