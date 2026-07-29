//! Regression budgets for `mailrs-spf`. See [BUDGETS.md](../BUDGETS.md).
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

use std::time::{Duration, Instant};

use mailrs_spf::Record;

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
fn parse_simple_under_budget() {
    let median = time_median(|| {
        let _ = Record::parse("v=spf1 ip4:203.0.113.0/24 -all");
    });
    // Budget: 5 µs (release ~80 ns).
    let budget = Duration::from_micros(5);
    assert!(median < budget, "parse_simple {median:?} > {budget:?}");
}

#[test]
fn parse_complex_under_budget() {
    let input = "v=spf1 ip4:203.0.113.0/24 ip4:198.51.100.0/24 ip6:2001:db8::/32 \
                 a:mail.example.com mx:example.com include:_spf.google.com \
                 include:spf.protection.outlook.com -all";
    let median = time_median(|| {
        let _ = Record::parse(input);
    });
    // Budget: 10 µs (release ~500 ns).
    let budget = Duration::from_micros(10);
    assert!(median < budget, "parse_complex {median:?} > {budget:?}");
}
