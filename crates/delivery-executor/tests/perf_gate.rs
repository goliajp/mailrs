//! Regression budgets for `mailrs-delivery-executor`. See BUDGETS.md.
//!
//! The executor itself is mostly I/O-bound (Maildir fsync, channel
//! send/recv). Real perf is exercised end-to-end by mailrs-server's
//! smtp_load bench (3.71× throughput win on top of maildir 1.2 — see
//! PERFORMANCE.md). The gate below catches "spawn became insanely
//! slow", which would point at a regression in the runtime setup
//! code rather than the hot path.

use std::time::{Duration, Instant};
use mailrs_delivery_executor::DeliveryExecutor;

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
