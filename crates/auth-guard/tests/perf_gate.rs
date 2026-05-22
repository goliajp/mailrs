//! Regression budgets. See [BUDGETS.md](../BUDGETS.md).

use std::net::IpAddr;
use std::time::{Duration, Instant};

use mailrs_auth_guard::{AuthCheck, AuthGuard, AuthGuardConfig};

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
fn check_empty_map_under_budget() {
    let guard = AuthGuard::new(AuthGuardConfig::default());
    let ip: IpAddr = "192.0.2.1".parse().unwrap();
    let median = time_median(|| {
        let r = guard.check(ip, "alice");
        assert!(matches!(r, AuthCheck::Allowed));
    });
    // Budget: 1 µs (release ~10 ns). Success path is two DashMap reads.
    let budget = Duration::from_micros(1);
    assert!(
        median < budget,
        "check_empty median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn check_locked_out_under_budget() {
    let guard = AuthGuard::new(AuthGuardConfig::default());
    let ip: IpAddr = "192.0.2.2".parse().unwrap();
    for _ in 0..10 {
        guard.record_failure(ip, "bob");
    }
    let median = time_median(|| {
        let r = guard.check(ip, "bob");
        assert!(matches!(r, AuthCheck::LockedOut { .. }));
    });
    // Budget: 2 µs (release ~30 ns). DashMap read + Instant::now arithmetic.
    let budget = Duration::from_micros(2);
    assert!(
        median < budget,
        "check_locked_out median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn record_failure_repeat_under_budget() {
    let guard = AuthGuard::new(AuthGuardConfig::default());
    let ip: IpAddr = "192.0.2.3".parse().unwrap();
    // prime
    guard.record_failure(ip, "carol");
    let median = time_median(|| {
        guard.record_failure(ip, "carol");
    });
    // Budget: 10 µs (release ~250 ns). DashMap get_mut + Vec::retain +
    // push + tracing::warn (which is no-op at info level).
    let budget = Duration::from_micros(10);
    assert!(
        median < budget,
        "record_failure_repeat median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn cleanup_stale_under_budget_1k_entries() {
    let guard = AuthGuard::new(AuthGuardConfig::default());
    // pre-populate 1000 entries
    for octet in 0..=255u8 {
        for low in 0..4u8 {
            let ip: IpAddr = format!("10.{octet}.0.{low}").parse().unwrap();
            guard.record_failure(ip, "x");
        }
    }
    let median = time_median(|| {
        // cleanup with current time (preserves all active records)
        guard.cleanup_stale(Instant::now());
    });
    // Budget: 5 ms for 1024 entries (release ~100 µs). Scales linearly.
    let budget = Duration::from_millis(5);
    assert!(
        median < budget,
        "cleanup_stale_1k median {median:?} exceeded {budget:?}"
    );
}
