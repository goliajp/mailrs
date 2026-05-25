//! DNSBL reverse-IP scoring re-exported from
//! [`mailrs-dnsbl`](https://crates.io/crates/mailrs-dnsbl).
//!
//! Carved into its own crate so consumers who only need DNSBL don't
//! transitively pull greylist + PTR + (under default features) the
//! Redis backing store.
//!
//! The public surface is intentionally unchanged from earlier shield
//! versions — anything that previously worked against
//! `mailrs_shield::dnsbl::*` continues to work.

pub use mailrs_dnsbl::{
    DnsblCache, DnsblResult, check_dnsbl, dnsbl_query, interpret_spamhaus, is_ipv6_dnsbl_supported,
    reverse_ipv4,
};
