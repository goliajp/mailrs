# mailrs-rate-limit

[![Crates.io](https://img.shields.io/crates/v/mailrs-rate-limit?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-rate-limit)
[![docs.rs](https://img.shields.io/docsrs/mailrs-rate-limit?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-rate-limit)
[![License](https://img.shields.io/crates/l/mailrs-rate-limit?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-rate-limit?style=flat-square)](https://crates.io/crates/mailrs-rate-limit)

Token-bucket rate limiting for Rust services. Single async trait
([`RateLimitStore`]) over `&str` keys with unix-second time, plus a
bundled in-process reference implementation ([`InMemoryRateLimitStore`])
that runs at sub-µs median on a hot key. Bring your own backend
(Redis, DynamoDB, memcached, ...) by implementing the trait.

Extracted from [mailrs] so any Rust service — SMTP, HTTP, gRPC,
streaming — can lean on the same token-bucket pattern that fronts a
production mail server's connect path, without inheriting opinions
about which storage backend, which clock, or which key shape to use.

## Why not `governor`?

[`governor`] is excellent for in-process GCRA rate limiting with
quotas-as-types. This crate aims at a different niche:

- **`&str` keys, not types.** Limit per-IP, per-user, per-API-key,
  per-endpoint, per-tenant — all from one store, picked at the
  call site. No type-parameter gymnastics.
- **Pluggable backend.** The trait surface is three methods. Wire
  your own Redis / Valkey / DynamoDB impl in ~80 lines.
- **Unix-second time in the trait boundary.** Portable across
  processes, languages, and clocks. Backends that prefer monotonic
  clocks internally (the bundled in-memory impl does) convert at
  the boundary.
- **Pure math exposed.** [`evaluate_bucket`] is the entire
  token-bucket arithmetic in one function. Call it directly if you
  want to plug it into a different store shape without going through
  the trait.

If you want a single-process limiter with statically-typed quotas,
use [`governor`]. If you want a swappable storage layer with `&str`
keys and a tiny surface, use this crate.

## Highlights

- **Backend-free trait.** Three async methods (`check`,
  `cleanup_stale`, `len`). Implementations choose their own
  storage and concurrency primitive.
- **Bundled in-memory impl.** [`InMemoryRateLimitStore`] is
  DashMap-backed; sub-µs `check` on a hot key (see
  [BUDGETS.md](BUDGETS.md)).
- **Pure math exposed.** [`evaluate_bucket`] turns
  `(bucket, now, config) → (next_bucket, allowed)` with no I/O, no
  allocations.
- **Stringly-typed keys.** `&str` accommodates every key shape
  (IPs, user IDs, API key prefixes, endpoint paths). One
  `.to_string()` at the boundary is the only conversion cost.
- **Perf-gated.** `tests/perf_gate.rs` asserts on µs-budget
  latency. Documented derivation in [BUDGETS.md](BUDGETS.md).

## Quick start

```rust
use mailrs_rate_limit::{InMemoryRateLimitStore, RateLimitStore, TokenBucketConfig};

# async fn run() {
let limiter = InMemoryRateLimitStore::new(TokenBucketConfig {
    capacity: 10,
    refill_rate: 1.0, // one token per second
});

// Allow the first 10 from this client in a burst...
for _ in 0..10 {
    assert!(limiter.check("203.0.113.42").await);
}
// ...then deny.
assert!(!limiter.check("203.0.113.42").await);

// A different client is unaffected.
assert!(limiter.check("198.51.100.7").await);

// Periodically (e.g. once an hour) drop idle keys to bound memory.
let one_hour_ago = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_secs()
    .saturating_sub(3600);
limiter.cleanup_stale(one_hour_ago).await;
# }
```

## Trait shape

```rust,ignore
#[async_trait]
pub trait RateLimitStore: Send + Sync {
    /// Try to consume one token for `key`. Returns `true` if allowed.
    async fn check(&self, key: &str) -> bool;

    /// Remove buckets that haven't been touched since `before_unix_secs`.
    async fn cleanup_stale(&self, before_unix_secs: u64);

    /// Approximate number of tracked keys (metrics).
    async fn len(&self) -> usize;
}
```

The trait makes no scheduling promises — call `cleanup_stale` on
whatever cadence makes sense for your traffic shape (once an hour
is typical for IP-keyed limiters; faster for sliding-window
analytics use cases).

## Bringing your own backend

A minimal Redis backend sketch (illustrative — wire your own
error handling and connection pooling):

```rust,ignore
use async_trait::async_trait;
use mailrs_rate_limit::{evaluate_bucket, Bucket, RateLimitStore, TokenBucketConfig};

pub struct RedisRateLimitStore {
    pool: redis::Pool,
    config: TokenBucketConfig,
}

#[async_trait]
impl RateLimitStore for RedisRateLimitStore {
    async fn check(&self, key: &str) -> bool {
        // 1. WATCH key
        // 2. GET key — parse Bucket from JSON or default to full
        // 3. let (next, allowed) = evaluate_bucket(bucket, now, &self.config);
        // 4. MULTI / SET key / EXEC
        // 5. retry on CAS conflict
        todo!()
    }
    async fn cleanup_stale(&self, _before: u64) { /* Redis EXPIRE handles this natively */ }
    async fn len(&self) -> usize { /* DBSIZE or SCAN */ 0 }
}
```

## What's in the box

- [`TokenBucketConfig`] — burst size + refill rate.
- [`Bucket`] — single-bucket state (tokens + last-refill timestamp).
- [`evaluate_bucket`] — pure token-bucket math.
- [`RateLimitStore`] — async trait for pluggable storage.
- [`InMemoryRateLimitStore`] — DashMap-backed reference impl.

## What's NOT in the box (intentionally)

- **No backends beyond in-memory.** Bring your own Redis, Valkey,
  DynamoDB, memcached impl. The trait is three methods.
- **No sliding-window log algorithm.** Token bucket only. If you
  need sliding window, implement your own
  [`RateLimitStore`] over the same trait.
- **No hierarchical buckets** (burst + sustained pair). Compose
  two stores at the call site; the crate does not do composition
  for you.
- **No per-key config.** One config per store instance. If you
  need tiers (e.g. auth at 10/min, general at 300/min), instantiate
  two stores.
- **No cleanup scheduler.** Call `cleanup_stale` from your own
  background task; the crate does not spawn anything.
- **No metrics emission.** `len()` is the hook — wire it into your
  dashboard.

These boundaries keep the crate tiny, framework-free, and trivial
to drop in next to whatever async runtime / DI container / metrics
stack you already use.

## Production reference

`mailrs` itself uses this crate at three call sites in its
SMTP / web stack:

- **Inbound SMTP connect** — capacity 10, refill 1/sec. Throttles
  abusive connect rates per source IP.
- **Web auth endpoints** — capacity 10, refill ~0.17/sec (10/minute).
  Slows brute-force password attacks.
- **Web general API** — capacity 300, refill 5/sec (300/minute).
  Backstop against runaway clients.

All three sit on top of the same `InMemoryRateLimitStore` shape,
keyed by `addr.ip().to_string()`.

## Tested

`1.0.0` ships **28 unit tests** + **8 trait-contract tests** +
**4 perf gates** across the three modules:

| Module | Tests | Surface |
| --- | ---: | --- |
| `config` | 3 | Default values, clone, zero-refill validity |
| `token_bucket` | 11 | Allow / reject / refill / cap / drain / clock backwards / boundary timestamps |
| `in_memory` | 14 | Sync + async API, per-key isolation, cleanup, concurrent access |
| `trait_contract` | 8 | Suite that any `RateLimitStore` impl should pass |
| `perf_gate` | 4 | `evaluate_bucket` + `check` (hot/cold) µs-budget |

Run with `cargo test -p mailrs-rate-limit`.

## Performance

Two layers of measurement:

- [`tests/perf_gate.rs`](tests/perf_gate.rs) — integration tests that fail CI if any hot path slows past its budget. See [BUDGETS.md](BUDGETS.md) for the full table.
- [`benches/store.rs`](benches/store.rs) — criterion microbenchmarks for detailed regression tracking.

Numbers below are criterion medians, measured with criterion 0.8 on Apple Silicon (M-series), release profile:

| Operation | Median | Notes |
|---|---|---|
| `evaluate_bucket` (allowed, pure math) | ~1.8 ns | branchless refill + decrement |
| `evaluate_bucket` (denied, no refill) | ~2.0 ns | exits without writing tokens |
| `InMemoryRateLimitStore::check_sync` (hot key) | ~41 ns | DashMap entry-lock + the math; dominated by `SystemTime::now()` |
| `InMemoryRateLimitStore::check` (async, hot key) | ~105 ns | + boxed-future overhead from `async-trait` |
| `InMemoryRateLimitStore::check_sync` (cold key, first touch) | ~180 ns | String alloc + DashMap insert |
| `cleanup_stale(10k entries, all stale)` | ~119 µs | full sweep, retain everything-or-nothing |
| `cleanup_stale(10k entries, none stale)` | ~119 µs | same cost — retain still touches every entry |

Run with `cargo bench -p mailrs-rate-limit`.

Comparison points: [`governor`] uses GCRA (more complex math but lock-free per-key), which is the right call when many keys all sit just below the limit at the same time. Token-bucket via DashMap is simpler and lets the bucket drain naturally, which matches typical anti-abuse usage (one key per IP / user). Pick by workload shape.

## Versioning

`1.x` follows semver. The stable public surface:

- [`RateLimitStore`] trait method signatures
- [`TokenBucketConfig`] struct shape
- [`Bucket`] struct shape (for backend authors)
- [`evaluate_bucket`] function signature + algorithm
- [`InMemoryRateLimitStore::new`] signature

The pure-math `evaluate_bucket` algorithm is frozen at 1.x — any
algorithm change is a major-version bump.

## License

Licensed under either [Apache License, Version 2.0](LICENSE-APACHE)
or [MIT license](LICENSE-MIT) at your option.

[mailrs]: https://github.com/goliajp/mailrs
[`governor`]: https://crates.io/crates/governor
[`RateLimitStore`]: https://docs.rs/mailrs-rate-limit/latest/mailrs_rate_limit/store/trait.RateLimitStore.html
[`TokenBucketConfig`]: https://docs.rs/mailrs-rate-limit/latest/mailrs_rate_limit/config/struct.TokenBucketConfig.html
[`Bucket`]: https://docs.rs/mailrs-rate-limit/latest/mailrs_rate_limit/token_bucket/struct.Bucket.html
[`evaluate_bucket`]: https://docs.rs/mailrs-rate-limit/latest/mailrs_rate_limit/token_bucket/fn.evaluate_bucket.html
[`InMemoryRateLimitStore`]: https://docs.rs/mailrs-rate-limit/latest/mailrs_rate_limit/in_memory/struct.InMemoryRateLimitStore.html
[`InMemoryRateLimitStore::new`]: https://docs.rs/mailrs-rate-limit/latest/mailrs_rate_limit/in_memory/struct.InMemoryRateLimitStore.html#method.new
