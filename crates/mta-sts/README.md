# mailrs-mta-sts

[![Crates.io](https://img.shields.io/crates/v/mailrs-mta-sts.svg)](https://crates.io/crates/mailrs-mta-sts)
[![Docs.rs](https://docs.rs/mailrs-mta-sts/badge.svg)](https://docs.rs/mailrs-mta-sts)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

RFC 8461 **MTA Strict Transport Security (MTA-STS)** — STS DNS record + HTTPS policy
parsers, MX pattern matching with `*.` wildcards, an `enforce(&Policy, mx_host)`
decision function, and a `Cache` trait with a tokio-friendly in-memory reference
implementation.

**Pure: no HTTP, no DNS.** This crate intentionally does no network I/O. You
bring your own DNS resolver and HTTPS client (hickory + reqwest, trust-dns +
hyper, whatever); when you have the bytes, you feed them in. That makes the
crate trivially testable, no async runtime required for the parsers, and lets
the caller decide its own caching, retry, and TLS policy.

## Why

MTA-STS lets a receiving domain advertise "you MUST use TLS, and you MUST
deliver to one of these MX hostnames" via a published policy file fetched
over HTTPS, anchored by a TXT record at `_mta-sts.<domain>`. The result is
defence against active downgrade attacks on inbound mail.

Existing Rust MTA-STS crates either (a) bundle their own HTTP client and DNS
resolver — opinions you may not share — or (b) live inside a bigger
mail-server framework. `mailrs-mta-sts` is the **parsers + decision logic**
in isolation, the same way `mailrs-spf` / `mailrs-dkim` / `mailrs-dmarc` /
`mailrs-arc` are split out.

## Quick start

```rust
use mailrs_mta_sts::{Policy, PolicyMode, StsRecord, enforce, mx_matches, policy_url, Decision};

// 1) Parse the STS TXT record (DNS).
let record = StsRecord::parse("v=STSv1; id=20200101T000000Z").unwrap();
assert_eq!(record.id, "20200101T000000Z");

// 2) Fetch the policy file. You did the HTTPS GET; we parse the body.
let body = "version: STSv1\nmode: enforce\nmx: *.mail.example.com\nmax_age: 604800\n";
let policy = Policy::parse(body).unwrap();
assert_eq!(policy.mode, PolicyMode::Enforce);

// 3) When picking an MX to deliver to, ask the policy.
match enforce(&policy, "mx1.mail.example.com") {
    Decision::Allow   => { /* proceed with delivery */ }
    Decision::Deny    => { /* abort: pattern mismatch in enforce mode */ }
    Decision::NoPolicy => { /* testing/none mode: don't block */ }
}

// 4) URL helper, in case you don't want to format it yourself.
assert_eq!(policy_url("example.com"), "https://mta-sts.example.com/.well-known/mta-sts.txt");
```

## What's in the box

| Module | Role |
|---|---|
| `record` | STS DNS TXT-record parser. `v=STSv1; id=...` |
| `policy` | Line-based policy-file parser. `version: STSv1` / `mode:` / `mx:` (repeatable) / `max_age:` |
| `enforce` | `enforce(&Policy, mx_host) -> Decision` + `mx_matches(host, pattern)` + `policy_url(domain)` |
| `cache` | `Cache` trait + `InMemoryCache` (tokio `RwLock<HashMap>`) for callers that don't want to wire their own |
| `error` | One `MtaStsError` enum covering both parsers |

The crate is **sync** for parsing and **async** only at the cache trait boundary.

## What's *not* in the box (and won't be)

- **No DNS lookup of `_mta-sts.<domain>`.** Use `hickory-resolver`, `trust-dns`,
  or `tokio::net::lookup_host`. Pass the TXT body to `StsRecord::parse`.
- **No HTTPS fetch of the policy file.** Use `reqwest`, `hyper`, `ureq`. Pass
  the body to `Policy::parse`. (Don't forget: the spec requires TLS validation
  against the policy host, NOT the apex domain.)
- **No TLSRPT.** That's a separate spec (RFC 8460) and would be a separate crate.

These are caller decisions. Forcing one choice on every caller is exactly the
trade-off `mailrs-mta-sts` was extracted to avoid.

## Performance

Microbenchmarks on Apple M-class silicon, release build. Run them yourself with
`cargo bench -p mailrs-mta-sts --bench mta_sts`:

| Operation                          | Time  |
|------------------------------------|-------|
| `StsRecord::parse` (1-tag TXT)     |  78 ns |
| `Policy::parse` (6-line, 3 MX)     | 321 ns |
| `mx_matches` literal               |  49 ns |
| `mx_matches` wildcard (match)      |  93 ns |
| `mx_matches` wildcard (no match)   | 100 ns |
| `enforce` 3-mx, first match        |  44 ns |
| `enforce` 3-mx, last match         | 223 ns |
| `enforce` 3-mx, no match → Deny    | 182 ns |

Parsers are zero-allocation for the STS record (one tiny `String` for the id)
and allocate only `Vec<String>` MX entries for the policy. The hot path
inside an outbound delivery (`enforce` after the policy is already loaded
from cache) is sub-microsecond.

Perf budgets are gated by `tests/perf_gate.rs` (runs every `cargo test`);
budget table lives in [`BUDGETS.md`](./BUDGETS.md).

## Cache trait

```rust
#[async_trait::async_trait]
pub trait Cache: Send + Sync {
    async fn get(&self, domain: &str) -> Option<CachedPolicy>;
    async fn put(&self, domain: &str, cached: CachedPolicy);
    async fn delete(&self, domain: &str);
}
```

Two reasons it exists:

1. The reference `InMemoryCache` is a `tokio::sync::RwLock<HashMap>`, sufficient
   for a single-process MTA. Plugging in Redis/Valkey/Memcached/sled is a
   matter of impl'ing this trait.
2. Tests for the enforce path don't need a real HTTP fetcher — they can fill
   the cache directly.

## License

Licensed under either of **Apache License, Version 2.0** ([LICENSE-APACHE](./LICENSE-APACHE))
or **MIT License** ([LICENSE-MIT](./LICENSE-MIT)) at your option.

## Part of `mailrs`

`mailrs-mta-sts` is one of the published email primitives carved out of the
[mailrs](https://github.com/goliajp/mailrs) mail server. Sister crates that
form the same family:

| Crate              | Role                                                    |
|--------------------|---------------------------------------------------------|
| `mailrs-spf`       | RFC 7208 SPF                                            |
| `mailrs-dkim`      | RFC 6376 DKIM (sign + verify, canon, tag-list)          |
| `mailrs-dmarc`     | RFC 7489 DMARC (policy, alignment, reporting)           |
| `mailrs-arc`       | RFC 8617 ARC (structural verify; crypto in 1.1)         |
| `mailrs-mta-sts`   | **This crate.** RFC 8461                                |
| `mailrs-mime`      | RFC 2045/2046 MIME parsing                              |
| `mailrs-smtp-proto`| RFC 5321 SMTP state machine + parser                    |

All sister crates share the "pure parsers + decision logic, no I/O" design —
bring your own resolver, your own TLS, your own storage.
