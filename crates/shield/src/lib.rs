//! SMTP anti-spam primitives: DNSBL, greylisting, and FCrDNS.
//!
//! Three independent modules used by inbound SMTP pipelines to filter
//! unwanted mail at connect / sender-evaluation time:
//!
//! - [`dnsbl`] — query DNS-based blocklists (Spamhaus, Barracuda, …)
//!   for an inbound client IP, with an in-process TTL cache.
//! - [`greylist`] — temporary 4xx-defer policy ([Harris 2003] / RFC
//!   6647): unknown sender triplets get one initial defer; legitimate
//!   senders retry, spammers don't. Comes with an optional Redis-backed
//!   store ([`greylist::GreylistDb`]) behind the default `redis-store`
//!   feature.
//! - [`ptr`] — forward-confirmed reverse DNS (FCrDNS) score for an
//!   inbound client: 0.0 if PTR → forward roundtrip matches the EHLO
//!   domain, 1.0 if it doesn't.
//!
//! ## Feature flags
//!
//! - `redis-store` (default) — enables the Redis-backed
//!   [`greylist::GreylistDb`] store (with optional Postgres cold backup).
//!   Disable to plug in your own store.
//!
//! [Harris 2003]: https://projects.puremagic.com/greylisting/

pub mod dnsbl;
pub mod greylist;
pub mod ptr;
