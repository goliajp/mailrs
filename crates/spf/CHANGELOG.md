# Changelog

All notable changes to `mailrs-spf` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.4] - 2026-05-23

### Changed

- `parse_mechanism` dispatch swapped from `match name` (UTF-8 string
  match) to `match name.as_bytes()` (byte literal match). Mechanism
  names are pure ASCII per RFC 7208 §5, so the byte form is strictly
  cheaper. Net effect on the hot path is sub-nanosecond on the simple
  case; documented for completeness.

### Performance

| Input | mailrs-spf 1.0.4 | mail-auth 0.9 |
|---|---:|---:|
| `v=spf1 ip4:... -all` (3 mech) | 63 ns | 50 ns |
| 8-mechanism complex | **360 ns** | 410 ns |
| 8-include pathological | **400 ns** | 577 ns |

Mail-auth still wins the 3-mech simple case by 13 ns due to its hand-
rolled byte-iter IPv4 parser (vs our `std::net::Ipv4Addr::FromStr`).
On anything realistic-sized — multi-mechanism, include-heavy — mailrs
wins by 14-44%.

## [1.0.3] - 2026-05-23

### Changed

- `parse_addr_and_prefix` returns `&str` instead of `String` for the address
  half — drops one allocation per `ip4:` / `ip6:` mechanism on the hot path.
  No public API change.

### Performance

Measured (criterion, M-series Mac, release, `--quick`):

| Input | Before | After | mail-auth 0.9 |
|---|---:|---:|---:|
| `v=spf1 ip4:... -all` (3 mech) | 71 ns | **62 ns** | 51 ns |
| 8-mechanism complex | 387 ns | **344 ns** | 400 ns |
| 8-include pathological | 375 ns | **379 ns** | 545 ns |

Net: mailrs-spf wins the realistic + pathological cases, loses by 11 ns
on tiny 3-mechanism records (mail-auth's byte-iter dispatch is tighter on
short inputs). Bench source: `benches/compare_mail_auth.rs`.

### Added

- `benches/compare_mail_auth.rs` — head-to-head parse bench against `mail-auth`.

## [1.0.2] - 2026-05-23

### Added

- `tests/perf_gate.rs` with 2 regression budgets (parse simple +
  parse complex 8-mechanism record).
- `BUDGETS.md` documenting the perf table + non-budgets.

No lib code change.

## [1.0.1] - 2026-05-23

### Changed

- README + workspace PERFORMANCE.md updated with measured criterion
  numbers from the bench harness (was: TBD).
  - `Record::parse` simple: 82 ns
  - `Record::parse` complex 8-mechanism: 484 ns
  - `verify` pass-path (no DNS): 244 ns

No code change.

## [1.0.0] - 2026-05-23

### Added

- Initial release. Carved out as the #1 candidate from the project's
  DEPS_AUDIT (replacing the SPF half of `mail-auth`).
- `Record::parse(&str) -> Result<Record, SpfError>` — RFC 7208 §4
  record parser.
- `Mechanism` enum: `All`, `Ip4`, `Ip6`, `A`, `Mx`, `Include`,
  `Exists` (RFC §5).
- `Qualifier` enum: `Pass` (default `+`), `Fail` (`-`),
  `SoftFail` (`~`), `Neutral` (`?`).
- `SpfResolver` async trait — pluggable DNS layer. Implementors map
  NXDOMAIN to `Ok(vec![])` (not error) so SPF "no record" =
  [`SpfResult::None`] flows correctly.
- `HickoryResolver` adapter behind the `hickory` feature (default on).
- `verify(resolver, input) -> SpfResult` — top-level evaluator.
- `VerifyInput { ip, helo, mail_from }` with `target_domain()` helper
  that falls back to HELO when MAIL FROM is empty (bounces).
- `SpfResult` enum with all seven RFC-prescribed values:
  `None / Pass / Fail / SoftFail / Neutral / PermError / TempError`
  with `as_str()` returning the lowercase wire form per RFC 7001.
- DNS lookup budget (≤10 per RFC §4.6.4) + recursion depth cap (10)
  for `include:` chains.
- Multi-`v=spf1` detection (RFC §4.5 → `PermError`).
- IPv4 + IPv6 subnet math with full prefix range (0-32, 0-128).
- 41 inline unit tests covering: parser edges (qualifiers, all
  mechanisms, prefixes, IPv6 `//N` syntax, modifier-skip),
  evaluator paths (pass/fail/softfail/neutral/none/permerror,
  IP4 + IP6 match, A + MX via DNS, include recursion, multi-record
  rejection).

### Out of scope (deferred — see README "What this crate does not")

- Macro expansion (RFC §7) — `%{i}`, `%{s}`, `%{d}` in `exists:` /
  `include:` templates. Common literal-domain records work; macro-
  heavy bulk-mailer records compute against the literal template.
- `redirect=` modifier (§6.1) — detected and skipped.
- `exp=` modifier (§6.2) — detected and skipped.
- `ptr` mechanism (§5.5) — deprecated per RFC; returns `PermError`.

These are intentional v1 scope limits; add as 1.x minors when needed.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-spf-v1.0.0...HEAD
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-spf-v1.0.0
