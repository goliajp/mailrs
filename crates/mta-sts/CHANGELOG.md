# Changelog

All notable changes to `mailrs-mta-sts` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.0.1] - 2026-06-03

### Changed

- Documentation only: the README and the `Cache` trait docs use kevy
  (<https://github.com/goliajp/kevy>), a RESP-compatible key-value store,
  as the example backend instead of Valkey/Redis. No API change.

## [2.0.0] - 2026-05-29

### Changed

- **Breaking:** `StsRecord::id`, `CachedPolicy::id` and the entries of
  `Policy::mx` are `compact_str::CompactString` instead of `String`.
  Hostnames and policy ids fit inline, which removes most allocations
  when parsing.
- New dependency: `compact_str` 0.9.

## [1.0.0] - 2026-05-23

Initial stable release. RFC 8461 MTA-STS parsers + decision logic,
extracted from the [mailrs](https://github.com/goliajp/mailrs) mail
server so other Rust MTAs can reuse a known-shape, no-I/O implementation.

### Parsers

- **STS DNS TXT record** (`StsRecord::parse`): `v=STSv1; id=...`.
  Forward-compatible — unknown tags are ignored (per spec); duplicate `v`
  rejected; `id` max-length 32 (spec); empty `id` rejected.
- **Policy file** (`Policy::parse`): line-based key:value, comments,
  blank lines, CRLF tolerated, case-insensitive header values, repeatable
  `mx:` lines, MX hostnames lowercased on ingest.

### Decision logic

- `mx_matches(host, pattern)` — exact match or single-label `*.` wildcard
  (RFC 8461 §4.1). Wildcard does not cross dots; trailing-dot tolerated on
  both sides.
- `enforce(&Policy, mx_host) -> Decision` — `Allow` / `Deny` / `NoPolicy`.
  `NoPolicy` is returned for `mode: testing` and `mode: none`, so callers
  can route them through the same code path without ad-hoc match arms.
- `policy_url(domain)` — RFC-required canonical URL helper.

### Cache

- `Cache` async trait (`async-trait`-based, `Send + Sync`).
- `InMemoryCache` reference impl backed by `tokio::sync::RwLock<HashMap>`.
- `CachedPolicy { id, policy, fetched_at_unix_secs }` so callers can apply
  `max_age` themselves without hauling around a separate map.

### Measured perf

Apple M-class silicon, release build, criterion microbenches:

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

### Tests

49 inline unit tests + 4 perf-gate integration tests. Two fuzz targets
(record + policy parsers) live in the repository under
`crates/mta-sts/fuzz/`; they are not part of the published crate.

### Pure-parser design

No HTTP client, no DNS resolver, no `tokio::net`. Caller does the network,
caller passes the bytes in. The only `tokio` dependency is `sync` (for the
in-memory cache's `RwLock`). Matches sister crates `mailrs-spf`,
`mailrs-dkim`, `mailrs-dmarc`, `mailrs-arc`.
