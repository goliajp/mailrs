//! Adapter: the server uses [`mailrs_rate_limit`] (1.0.0) with
//! IP-keyed buckets at the SMTP connect path and at the web API
//! middleware. This module is a thin shim over the published crate;
//! the historical sync `check(IpAddr)` API has been replaced by the
//! crate's `check(&str).await` boundary.
//!
//! Behavior is identical to the pre-extraction version:
//!
//! - Same `TokenBucketConfig` shape and defaults.
//! - Fresh keys start at full capacity.
//! - `cleanup_stale` uses unix-seconds; the server passes
//!   `SystemTime::now() - 3600s` as the threshold.

pub use mailrs_rate_limit::{InMemoryRateLimitStore, RateLimitStore, TokenBucketConfig};

/// Public alias kept for source-compatibility with the rest of the
/// server crate. Prefer [`InMemoryRateLimitStore`] in new code.
pub type RateLimiter = InMemoryRateLimitStore;
