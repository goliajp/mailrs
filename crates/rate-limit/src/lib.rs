#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![doc = include_str!("../README.md")]

//! Token-bucket rate limiting for Rust services.
//!
//! See the crate-level README for design rationale and worked examples.

pub mod config;
pub mod in_memory;
pub mod store;
pub mod token_bucket;

pub use config::TokenBucketConfig;
pub use in_memory::InMemoryRateLimitStore;
pub use store::RateLimitStore;
pub use token_bucket::{Bucket, evaluate_bucket};
