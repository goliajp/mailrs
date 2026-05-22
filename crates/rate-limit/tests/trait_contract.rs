//! Trait conformance suite. Any [`RateLimitStore`] impl should satisfy
//! every test here. New backend impls (Redis, DynamoDB, ...) added in
//! future versions plug into [`run_contract_tests`] and get the same
//! coverage for free.

use mailrs_rate_limit::{InMemoryRateLimitStore, RateLimitStore, TokenBucketConfig};

/// Tests that any compliant `RateLimitStore` must pass.
///
/// Each test calls the constructor it needs (different configs for
/// different scenarios), so the test driver below just invokes them
/// all in sequence. Implementations that need async setup should
/// adapt this pattern with their own constructor wrapper.
async fn allow_within_capacity<S: RateLimitStore>(store: S) {
    // capacity = 3, refill = 0 — exactly 3 allowed, 4th denied.
    assert!(store.check("client").await);
    assert!(store.check("client").await);
    assert!(store.check("client").await);
    assert!(!store.check("client").await);
}

async fn reject_over_capacity<S: RateLimitStore>(store: S) {
    // capacity = 1, refill = 0 — first allowed, all subsequent denied.
    assert!(store.check("client").await);
    for _ in 0..10 {
        assert!(!store.check("client").await);
    }
}

async fn per_key_isolation<S: RateLimitStore>(store: S) {
    // capacity = 1, refill = 0 — each key has its own bucket.
    assert!(store.check("a").await);
    assert!(!store.check("a").await);
    assert!(store.check("b").await);
    assert!(!store.check("b").await);
    assert!(store.check("c").await);
    assert!(!store.check("c").await);
}

async fn first_check_per_key_is_allowed<S: RateLimitStore>(store: S) {
    // capacity = 1 means each fresh key gets exactly one allowed
    // request, regardless of how many keys we hit.
    for i in 0..50 {
        let key = format!("k{i}");
        assert!(
            store.check(&key).await,
            "first check for fresh key {key} must be allowed"
        );
    }
}

async fn cleanup_then_len_reports_zero<S: RateLimitStore>(store: S) {
    for i in 0..5 {
        let key = format!("k{i}");
        store.check(&key).await;
    }
    assert_eq!(store.len().await, 5);

    // far-future cutoff drops everything
    store.cleanup_stale(u64::MAX).await;
    assert_eq!(store.len().await, 0);
}

async fn len_reflects_unique_keys<S: RateLimitStore>(store: S) {
    assert_eq!(store.len().await, 0);

    store.check("a").await;
    assert_eq!(store.len().await, 1);

    store.check("b").await;
    assert_eq!(store.len().await, 2);

    // same key again — len unchanged
    store.check("a").await;
    assert_eq!(store.len().await, 2);

    store.check("c").await;
    assert_eq!(store.len().await, 3);
}

async fn cleanup_with_no_keys_is_noop<S: RateLimitStore>(store: S) {
    store.cleanup_stale(0).await;
    store.cleanup_stale(u64::MAX).await;
    assert_eq!(store.len().await, 0);
}

async fn check_after_full_cleanup_creates_fresh_bucket<S: RateLimitStore>(store: S) {
    // drain key
    assert!(store.check("k").await);
    assert!(store.check("k").await);
    assert!(!store.check("k").await);

    // cleanup drops it
    store.cleanup_stale(u64::MAX).await;

    // fresh bucket at full capacity (2) — two more allowed
    assert!(store.check("k").await);
    assert!(store.check("k").await);
    assert!(!store.check("k").await);
}

// ---- Driver tests for InMemoryRateLimitStore ----

fn drained_after_three() -> InMemoryRateLimitStore {
    InMemoryRateLimitStore::new(TokenBucketConfig {
        capacity: 3,
        refill_rate: 0.0,
    })
}

fn capacity_one_no_refill() -> InMemoryRateLimitStore {
    InMemoryRateLimitStore::new(TokenBucketConfig {
        capacity: 1,
        refill_rate: 0.0,
    })
}

fn capacity_two_no_refill() -> InMemoryRateLimitStore {
    InMemoryRateLimitStore::new(TokenBucketConfig {
        capacity: 2,
        refill_rate: 0.0,
    })
}

fn default_config_store() -> InMemoryRateLimitStore {
    InMemoryRateLimitStore::new(TokenBucketConfig::default())
}

#[tokio::test]
async fn in_memory_allow_within_capacity() {
    allow_within_capacity(drained_after_three()).await;
}

#[tokio::test]
async fn in_memory_reject_over_capacity() {
    reject_over_capacity(capacity_one_no_refill()).await;
}

#[tokio::test]
async fn in_memory_per_key_isolation() {
    per_key_isolation(capacity_one_no_refill()).await;
}

#[tokio::test]
async fn in_memory_first_check_per_key_is_allowed() {
    first_check_per_key_is_allowed(capacity_one_no_refill()).await;
}

#[tokio::test]
async fn in_memory_cleanup_then_len_reports_zero() {
    cleanup_then_len_reports_zero(default_config_store()).await;
}

#[tokio::test]
async fn in_memory_len_reflects_unique_keys() {
    len_reflects_unique_keys(default_config_store()).await;
}

#[tokio::test]
async fn in_memory_cleanup_with_no_keys_is_noop() {
    cleanup_with_no_keys_is_noop(default_config_store()).await;
}

#[tokio::test]
async fn in_memory_check_after_full_cleanup_creates_fresh_bucket() {
    check_after_full_cleanup_creates_fresh_bucket(capacity_two_no_refill()).await;
}
