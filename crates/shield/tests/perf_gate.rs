//! Performance regression gates. See [BUDGETS.md](../BUDGETS.md).

use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use mailrs_shield::dnsbl::{interpret_spamhaus, reverse_ipv4};
use mailrs_shield::greylist::{GreylistConfig, evaluate_triplet, triplet_key};
use mailrs_shield::ptr::ptr_score_from_names;

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
fn dnsbl_reverse_ipv4_under_budget() {
    let median = time_median(|| {
        let _ = reverse_ipv4(Ipv4Addr::new(1, 2, 3, 4));
    });
    // Budget: 10 µs. Observed P95: ~100 ns.
    let budget = Duration::from_micros(10);
    assert!(median < budget, "reverse_ipv4 median {median:?} exceeded {budget:?}");
}

#[test]
fn dnsbl_interpret_spamhaus_under_budget() {
    let median = time_median(|| {
        let _ = interpret_spamhaus(Ipv4Addr::new(127, 0, 0, 2));
    });
    // Budget: 10 µs. Observed P95: ~50 ns.
    let budget = Duration::from_micros(10);
    assert!(median < budget, "interpret_spamhaus median {median:?} exceeded {budget:?}");
}

#[test]
fn greylist_evaluate_retry_under_budget() {
    let cfg = GreylistConfig::default();
    let median = time_median(|| {
        let _ = evaluate_triplet(Some(1000), 2000, &cfg);
    });
    // Budget: 10 µs. Observed P95: ~50 ns.
    let budget = Duration::from_micros(10);
    assert!(median < budget, "evaluate_triplet median {median:?} exceeded {budget:?}");
}

#[test]
fn greylist_triplet_key_under_budget() {
    let median = time_median(|| {
        let _ = triplet_key("192.0.2.1", "alice@example.com", "bob@example.org");
    });
    // Budget: 50 µs. Observed P95: ~500 ns (sha + alloc).
    let budget = Duration::from_micros(50);
    assert!(median < budget, "triplet_key median {median:?} exceeded {budget:?}");
}

#[test]
fn ptr_score_match_under_budget() {
    let names = vec![
        "mail.example.com.".into(),
        "mx1.example.com.".into(),
        "mx2.example.com.".into(),
    ];
    let median = time_median(|| {
        let _ = ptr_score_from_names(&names, "example.com");
    });
    // Budget: 20 µs. Observed P95: ~200 ns.
    let budget = Duration::from_micros(20);
    assert!(median < budget, "ptr_score_from_names median {median:?} exceeded {budget:?}");
}
