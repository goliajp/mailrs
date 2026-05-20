# mailrs-shield

[![Crates.io](https://img.shields.io/crates/v/mailrs-shield?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-shield)
[![docs.rs](https://img.shields.io/docsrs/mailrs-shield?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-shield)
[![License](https://img.shields.io/crates/l/mailrs-shield?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-shield?style=flat-square)](https://crates.io/crates/mailrs-shield)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

SMTP server anti-spam primitives in three modules: **DNSBL** lookups, **greylisting** policy, and **FCrDNS** (forward-confirmed reverse DNS) checks — async, transport-agnostic, mostly zero-I/O.

Extracted from [mailrs] so any Rust mail server can drop these in without re-implementing the same DNS-walking patterns. The Rust ecosystem currently has no dedicated crates for any of these three primitives.

## What's inside

### `shield::dnsbl` — DNS blocklist queries

Look up an inbound client IP against zones like Spamhaus ZEN, Barracuda, etc. Comes with an in-process TTL cache so repeat connections don't re-query.

```rust,no_run
use hickory_resolver::TokioResolver;
use mailrs_shield::dnsbl::check_dnsbl;
use std::net::IpAddr;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let resolver = TokioResolver::builder_tokio()?.build()?;
let ip: IpAddr = "203.0.113.42".parse()?;
let zones = &["zen.spamhaus.org".to_string(), "b.barracudacentral.org".to_string()];
let result = check_dnsbl(&resolver, ip, zones).await;
println!("{result:?}");
# Ok(())
# }
```

### `shield::greylist` — Greylisting policy

Pure policy ([Harris 2003] / RFC 6647): defer the first time you see a `(client_ip, sender, recipient)` triplet, accept after the configured delay if the sender retries. Legitimate MTAs queue and retry; most spam bots don't.

```rust
use mailrs_shield::greylist::{GreylistConfig, GreylistDecision, evaluate_triplet};

let cfg = GreylistConfig::default();   // 5-minute initial delay, 36-day pass window
assert_eq!(evaluate_triplet(None, 1000, &cfg), GreylistDecision::Defer);
assert_eq!(evaluate_triplet(Some(1000), 1100, &cfg), GreylistDecision::TooEarly);
assert_eq!(evaluate_triplet(Some(1000), 1400, &cfg), GreylistDecision::Accept);
```

The optional `redis-store` feature (on by default) ships a `GreylistDb` that combines Redis (hot cache) + Postgres (cold backup) behind a single `check()` call:

```rust,no_run
# #[cfg(feature = "redis-store")]
# async fn _ex() -> Result<(), Box<dyn std::error::Error>> {
use mailrs_shield::greylist::{GreylistConfig, GreylistDb, triplet_key};

let cm = redis::aio::ConnectionManager::new(
    redis::Client::open("redis://localhost")?
).await?;
let db = GreylistDb::new(cm);
let cfg = GreylistConfig::default();
let key = triplet_key("192.0.2.1", "alice@example.com", "bob@example.com");
let now = 1700000000;
let decision = db.check(&key, now, &cfg).await;
# Ok(())
# }
```

Disable the feature to plug in your own store — the trait surface is just "given a key + clock, look up the first-seen timestamp."

### `shield::ptr` — FCrDNS check

Score an inbound client by whether its IP's reverse DNS forward-resolves back to a name matching the EHLO domain. Returns `0.0` on full match, `1.0` on no match — easy to fold into a spam score.

```rust,no_run
use hickory_resolver::TokioResolver;
use mailrs_shield::ptr::check_client_ptr;
use std::net::IpAddr;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let resolver = TokioResolver::builder_tokio()?.build()?;
let ip: IpAddr = "192.0.2.1".parse()?;
let score = check_client_ptr(&resolver, ip, "mta.example.com").await;
println!("{score:.1}");
# Ok(())
# }
```

## Feature flags

| Flag | Default | What it enables |
|------|---------|-----------------|
| `redis-store` | yes | `greylist::GreylistDb` (Redis + optional PG cold backup) |

Disable both default features (`default-features = false`) if you're plugging in your own backends.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

[mailrs]: https://github.com/goliajp/mailrs
[Harris 2003]: https://projects.puremagic.com/greylisting/
