//! Regression budgets for `mailrs-delivery-executor`. See BUDGETS.md.
//!
//! The executor itself is mostly I/O-bound (Maildir fsync, channel
//! send/recv). Real perf is exercised end-to-end by mailrs-server's
//! smtp_load bench (3.71× throughput win on top of maildir 1.2 — see
//! PERFORMANCE.md). The gate below catches "spawn became insanely
//! slow", which would point at a regression in the runtime setup
//! code rather than the hot path.
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

use mailrs_delivery_executor::DeliveryExecutor;
use std::time::{Duration, Instant};

#[tokio::test]
async fn spawn_under_budget() {
    let start = Instant::now();
    let _ex = DeliveryExecutor::spawn();
    let elapsed = start.elapsed();
    // Budget: 5 ms — spawn does an mpsc channel + tokio::spawn.
    assert!(
        elapsed < Duration::from_millis(5),
        "DeliveryExecutor::spawn took {elapsed:?} (>5ms)"
    );
}
