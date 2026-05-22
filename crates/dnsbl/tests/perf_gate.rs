//! Regression budgets. See [BUDGETS.md](../BUDGETS.md).

use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use mailrs_dnsbl::{dnsbl_query, interpret_spamhaus, reverse_ipv4};

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
fn reverse_ipv4_under_budget() {
    let ip = Ipv4Addr::new(203, 0, 113, 42);
    let median = time_median(|| {
        let _ = reverse_ipv4(ip);
    });
    // Budget: 5 µs (release ~14 ns).
    let budget = Duration::from_micros(5);
    assert!(median < budget, "reverse_ipv4 median {median:?} > {budget:?}");
}

#[test]
fn dnsbl_query_under_budget() {
    let r = reverse_ipv4(Ipv4Addr::new(203, 0, 113, 42));
    let median = time_median(|| {
        let _ = dnsbl_query(&r, "zen.spamhaus.org");
    });
    // Budget: 5 µs (release ~25 ns). Pre-sized String + 2 push_str.
    let budget = Duration::from_micros(5);
    assert!(median < budget, "dnsbl_query median {median:?} > {budget:?}");
}

#[test]
fn interpret_spamhaus_under_budget() {
    let ip = Ipv4Addr::new(127, 0, 0, 2);
    let median = time_median(|| {
        let _ = interpret_spamhaus(ip);
    });
    // Budget: 1 µs (release ~700 ps). Pure match arm on a byte.
    let budget = Duration::from_micros(1);
    assert!(median < budget, "interpret_spamhaus median {median:?} > {budget:?}");
}
