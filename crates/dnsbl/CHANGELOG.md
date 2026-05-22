# Changelog

All notable changes to `mailrs-dnsbl` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0] - 2026-05-22

### Added

- Initial release. Code carved verbatim out of `mailrs-shield`'s
  `dnsbl` module (which ran in production for ~1 year). Existing
  users of `mailrs_shield::dnsbl::*` continue to work via shield's
  1.0.4 re-export shim.
- `reverse_ipv4(ip)` — RFC 5782 §2.1 canonical reverse-octet form.
  Replaces the original `format!()` with a pre-sized String for
  measurable savings on the hot inbound path.
- `dnsbl_query(reversed, zone)` — same change, pre-sized String.
- `DnsblResult` enum with documented Spamhaus return-code variants
  (Sbl, Css, Xbl, Pbl) + `Listed(other)` fallback for other operators.
- `interpret_spamhaus` — exhaustive 127.0.0.x → DnsblResult mapping.
- `check_dnsbl(resolver, ip, zones)` — sequential fan-out lookup;
  first-listed-zone wins.
- `DnsblCache` — TTL-cached wrapper; caches both positive and
  negative results.
- `is_ipv6_dnsbl_supported` — documented stub (always false).
- 22 inline unit tests covering: standard / zero / broadcast IP
  reversal, trailing-dot zone, all Spamhaus code variants (Sbl, Css,
  Xbl range 4..=7, Pbl 10/11), almost-127.x boundary cases,
  unknown-code fallthrough, cache cleanup preserves fresh entries,
  double-lookup returns same.
- `tests/perf_gate.rs` with 3 regression budgets.
- `benches/dnsbl.rs` with 6 criterion benchmark functions.

### Migration note

If you were previously importing from `mailrs_shield::dnsbl::*`,
nothing changes — `mailrs-shield` 1.0.4 re-exports this crate's
public surface unchanged. You can switch to `mailrs_dnsbl::*` at any
time; both paths resolve to the same types.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-dnsbl-v1.0.0...HEAD
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-dnsbl-v1.0.0
