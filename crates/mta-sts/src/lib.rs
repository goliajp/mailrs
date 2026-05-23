#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! RFC 8461 MTA Strict Transport Security (MTA-STS).
//!
//! MTA-STS lets a receiving domain declare that incoming mail must be
//! delivered over an authenticated TLS connection to one of a named
//! set of MX hostnames. A sending MTA that supports STS will:
//!
//! 1. Look up `_mta-sts.<recipient-domain>` TXT for a record like
//!    `v=STSv1; id=20200101T000000Z`. ([`StsRecord::parse`])
//! 2. If the `id` has changed since last fetch, GET the policy from
//!    `https://mta-sts.<recipient-domain>/.well-known/mta-sts.txt`.
//! 3. Parse the policy body. ([`Policy::parse`])
//! 4. For each MX returned by normal DNS, check that the MX hostname
//!    matches one of the policy's `mx:` patterns. ([`mx_matches`])
//! 5. If a match is found, attempt delivery with TLS certificate
//!    verification. If no match is found, the per-policy `mode`
//!    decides: `enforce` → don't deliver, `testing` → deliver and
//!    optionally report, `none` → ignore policy.
//!    ([`enforce`])
//!
//! ## Scope (1.0)
//!
//! This crate is **pure** — no HTTP, no DNS, no clock. The caller
//! brings:
//!
//! - a DNS layer for the TXT lookup (typically `mailrs-dns`),
//! - an HTTPS client for the policy fetch (typically `reqwest`),
//! - a clock for `max_age` enforcement,
//! - a [`Cache`] impl for the `id → Policy` mapping. We ship an
//!   in-memory ref impl ([`InMemoryCache`]).
//!
//! That keeps the stone bounded by RFC 8461 and reusable in any
//! async stack.

pub mod cache;
pub mod enforce;
pub mod error;
pub mod policy;
pub mod record;

pub use cache::{Cache, CachedPolicy, InMemoryCache};
pub use enforce::{enforce, mx_matches, policy_url, Decision};
pub use error::MtaStsError;
pub use policy::{Policy, PolicyMode};
pub use record::StsRecord;
