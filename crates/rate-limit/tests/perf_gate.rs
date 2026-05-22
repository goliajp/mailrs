//! Performance regression gates. See [BUDGETS.md](../BUDGETS.md).
//!
//! Every gated path here runs on the inbound critical line — between
//! the TCP accept and the protocol greeting (SMTP `220` / IMAP `OK`).
//! Rate limiting that takes more than a few µs per check defeats the
//! point of rate limiting at all.

use std::time::{Duration, Instant};

use mailrs_rate_limit::{
    Bucket, InMemoryRateLimitStore, RateLimitStore, TokenBucketConfig, evaluate_bucket,
};

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
fn evaluate_bucket_pure_math_under_budget() {
    let config = TokenBucketConfig {
        capacity: 10,
        refill_rate: 1.0,
    };
    let bucket = Bucket {
        tokens: 5.0,
        last_refill_unix_secs: 1_000_000,
    };
    let median = time_median(|| {
        let _ = evaluate_bucket(bucket, 1_000_001, &config);
    });
    // Budget: 5 µs (~30× headroom over observed ~10 ns).
    let budget = Duration::from_micros(5);
    assert!(
        median < budget,
        "evaluate_bucket median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn in_memory_check_hot_key_under_budget() {
    let store = InMemoryRateLimitStore::new(TokenBucketConfig {
        capacity: 1_000_000,
        refill_rate: 1_000_000.0,
    });
    // warm the entry once
    let _ = store.check_sync("hot-key");

    let median = time_median(|| {
        let _ = store.check_sync("hot-key");
    });
    // Budget: 30 µs (~30× headroom over observed ~1 µs on a hot key).
    // The DashMap entry-lock + the SystemTime::now() syscall dominate.
    let budget = Duration::from_micros(30);
    assert!(
        median < budget,
        "InMemoryRateLimitStore::check_sync (hot key) median {median:?} exceeded {budget:?}"
    );
}

#[tokio::test]
async fn in_memory_check_async_hot_key_under_budget() {
    let store = InMemoryRateLimitStore::new(TokenBucketConfig {
        capacity: 1_000_000,
        refill_rate: 1_000_000.0,
    });
    // warm the entry once
    let _ = store.check("hot-key").await;

    let mut samples = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let start = Instant::now();
        let _ = store.check("hot-key").await;
        samples.push(start.elapsed());
    }
    samples.sort();
    let median = samples[ITERS / 2];

    // Budget: 50 µs (~30× headroom). Async wrapper adds ~1 µs of
    // boxed-future overhead on top of the sync path. The budget
    // ensures any future regression (e.g. allocating a fresh closure
    // per call, switching to an unbounded channel) is caught.
    let budget = Duration::from_micros(50);
    assert!(
        median < budget,
        "InMemoryRateLimitStore::check (async hot key) median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn in_memory_check_cold_key_under_budget() {
    // Cold-key path: each check is a fresh DashMap insert + String
    // allocation. Budgeted higher than hot-key because alloc dominates.
    let store = InMemoryRateLimitStore::new(TokenBucketConfig {
        capacity: 10,
        refill_rate: 1.0,
    });
    let keys: Vec<String> = (0..ITERS).map(|i| format!("cold-{i}")).collect();

    let mut samples = Vec::with_capacity(ITERS);
    for k in &keys {
        let start = Instant::now();
        let _ = store.check_sync(k);
        samples.push(start.elapsed());
    }
    samples.sort();
    let median = samples[ITERS / 2];

    // Budget: 50 µs (~15× headroom over observed ~3 µs). Cold-key adds
    // String allocation + DashMap entry insertion (which may resize
    // the underlying shards). Real production traffic is mostly
    // hot-key after warm-up.
    let budget = Duration::from_micros(50);
    assert!(
        median < budget,
        "InMemoryRateLimitStore::check_sync (cold key) median {median:?} exceeded {budget:?}"
    );
}
