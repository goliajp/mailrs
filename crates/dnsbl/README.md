# mailrs-dnsbl

[![Crates.io](https://img.shields.io/crates/v/mailrs-dnsbl?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-dnsbl)
[![docs.rs](https://img.shields.io/docsrs/mailrs-dnsbl?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-dnsbl)
[![License](https://img.shields.io/crates/l/mailrs-dnsbl?style=flat-square)](#license)

RFC 5782 DNS-based blocklist (DNSBL) lookup: reverse-IPv4 query
construction, Spamhaus return-code interpretation, and an in-process
TTL cache covering both positive and negative hits.

```text
For inbound IP 1.2.3.4 against zone "zen.spamhaus.org":
  query  → 4.3.2.1.zen.spamhaus.org (A record)
  result → 127.0.0.2  →  DnsblResult::Sbl  (Spamhaus Block List)
           127.0.0.4  →  DnsblResult::Xbl  (Exploits Block List)
           NXDOMAIN   →  None              (not listed → Clean)
```

Carved out of `mailrs-shield`'s `dnsbl` module — same code, same API,
its own crate so users who only want DNSBL don't pull greylist + PTR
+ Postgres dependencies.

## Quickstart

```rust,no_run
use hickory_resolver::TokioResolver;
use mailrs_dnsbl::DnsblCache;
use std::net::IpAddr;
use std::time::Duration;

# async fn run(resolver: &TokioResolver, ip: IpAddr) {
let cache = DnsblCache::new(Duration::from_secs(300));
let zones = vec!["zen.spamhaus.org".into(), "bl.spamcop.net".into()];

match cache.check(resolver, ip, &zones).await {
    Some((zone, result)) => {
        eprintln!("listed in {zone}: {result:?}");
        // Reject the connection / score it
    }
    None => {
        // Not listed by any zone → accept
    }
}
# }
```

## What this crate does

- **`reverse_ipv4(ip)`** — `1.2.3.4 → "4.3.2.1"`, the canonical RFC
  5782 §2.1 query form
- **`dnsbl_query(reversed, zone)`** — `"4.3.2.1.zen.spamhaus.org"`
- **`interpret_spamhaus(reply)`** — map `127.0.0.x` reply codes to a
  typed `DnsblResult`. Sbl / Css / Xbl / Pbl + a `Listed(other)` fallback
  for non-Spamhaus DNSBLs sharing the 127.0.0.x convention
- **`check_dnsbl(resolver, ip, zones)`** — fan-out lookup, returns
  the FIRST zone that lists the IP (early exit, doesn't continue)
- **`DnsblCache`** — TTL-cached wrapper around `check_dnsbl`. Caches
  both positive and negative lookups; `cleanup()` drops expired entries
- **`is_ipv6_dnsbl_supported`** — stub returning `false`. Most DNSBL
  operators don't support v6 lookups; the function is the extension
  point for the day they do.

## What this crate does not

- **No DNS resolver of its own.** Bring `hickory-resolver` and pass it
  to `check_dnsbl` / `DnsblCache::check`. That's a deliberate choice:
  if your app already has a configured resolver (with timeouts, retry
  policy, DoH/DoT, etc.), this crate uses it. Picking the resolver
  for you would be opinionated.
- **No URI BL** (RFC 5782 §2.2 — querying for URIs found in body
  content). That's a different lookup shape; out of scope for 1.x.
- **No URIBL caching semantics beyond TTL**. If you want pluggable
  storage (Redis, Postgres, …) for the cache, wrap `check_dnsbl`
  yourself with whatever store you like.

## Performance

Measured (criterion, M-series Mac, release, 100-sample median):

| Operation | Median |
|---|---:|
| `reverse_ipv4` | **~14 ns** |
| `dnsbl_query` (~20-char zone) | **~25 ns** |
| `interpret_spamhaus` (Sbl reply) | **~700 ps** |
| `interpret_spamhaus` (non-127.x → Clean) | **~700 ps** |
| `DnsblCache::check` (cached hit) | **~80 ns** |

Note `check_dnsbl` itself is DNS-bound — milliseconds, not nanoseconds.
The bench numbers above are for the CPU pieces. The cache HIT path is
what matters for inbound throughput: a typical inbound run sees the
same IP repeatedly within a session, so cache hit ≈ 80 ns vs. cache
miss ≈ DNS RTT.

Reproduce: `cargo bench -p mailrs-dnsbl --bench dnsbl`. Workspace
[PERFORMANCE.md](../../PERFORMANCE.md) carries the same table.

## License

Apache-2.0 OR MIT.
