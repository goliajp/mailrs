#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! SMTP anti-spam primitives: DNSBL, greylisting, and FCrDNS.
//!
//! Three independent modules used by inbound SMTP pipelines to filter
//! unwanted mail at connect / sender-evaluation time:
//!
//! - [`dnsbl`] — query DNS-based blocklists (Spamhaus, Barracuda, …)
//!   for an inbound client IP, with an in-process TTL cache.
//! - [`greylist`] — temporary 4xx-defer policy ([Harris 2003] / RFC
//!   6647): unknown sender triplets get one initial defer; legitimate
//!   senders retry, spammers don't. [`greylist::GreylistDb`] runs the
//!   policy over a pluggable [`greylist::GreylistBackend`]; an in-process
//!   kevy backend ships behind the default `kevy-store` feature, and
//!   [`greylist::check_greylist_backend`] checks your own.
//! - [`ptr`] — forward-confirmed reverse DNS (FCrDNS) score for an
//!   inbound client: 0.0 if PTR → forward roundtrip matches the EHLO
//!   domain, 1.0 if it doesn't.
//!
//! ## Feature flags
//!
//! - `store` — [`greylist::GreylistDb`], the [`greylist::GreylistBackend`]
//!   trait and its contract check, with no storage dependency.
//! - `kevy-store` (default, implies `store`) — adds
//!   [`greylist::EmbeddedGreylistBackend`] over an in-process kevy store.
//!
//! [Harris 2003]: https://projects.puremagic.com/greylisting/

/// DNSBL reverse-IP scoring helpers (Spamhaus + generic lookup interpretation).
pub mod dnsbl;
/// RFC 6647 greylisting: defer-on-first-seen over a pluggable backend.
pub mod greylist;
/// The kevy key a greylist triplet is stored under.
pub mod greylist_key;
/// Forward-confirmed reverse DNS scoring (FCrDNS) for sender-IP reputation.
pub mod ptr;
