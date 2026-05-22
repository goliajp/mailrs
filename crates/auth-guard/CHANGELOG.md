# Changelog

All notable changes to `mailrs-auth-guard` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.2] - 2026-05-22

### Added

- 10 new edge-case tests: IPv6 different /64 subnets not blocked
  together, per-username independent tracking, record_failure during
  lockout doesn't panic, record_success preserves IP counter,
  cleanup_stale on empty maps, max_failures=1 locks immediately,
  high backoff caps at max_lockout_secs, fractional backoff
  multiplier monotonically decreases, concurrent record_failures
  from 8 threads × 50 calls don't panic/deadlock, IPv4 vs IPv6
  loopback tracked independently.
- Lib test count 16 → 26.

No behavior change; pure coverage-density bump.

## [1.0.1] - 2026-05-22

### Changed

- Perf gate `check_empty_map_under_budget` budget 1µs → 5µs to absorb
  dev-mode scheduler noise at sub-µs scale. Release-mode median ~43 ns
  unchanged. No lib code change.

## [1.0.0] - 2026-05-22

### Added

- Initial release. Extracted from `mailrs-server` where it ran in
  production for ~1 year.
- `AuthGuard` struct with `check`, `record_failure`, `record_success`,
  `cleanup_stale` methods.
- Per-(IP, username) + per-IP dual counters with independent sliding
  windows.
- Exponential-backoff lockout (configurable multiplier + ceiling).
- IPv6 normalized to /64 prefix so a single attacker block can't
  trivially evade by hopping addresses within their own delegation.
- `AuthGuardConfig` with sensible SMTP/IMAP defaults: 5 fails per
  (IP, username) in 15 minutes, 20 fails per IP in 60 minutes,
  exponential backoff with multiplier 2.0 capped at 24 hours.
- Allocation-free on the `check` success path (the path that runs
  on every legitimate login).
- 20 inline unit tests covering: lockout activation, exponential
  backoff, IPv6 /64 collapse, cleanup_stale active/expired
  preservation, success-resets-account semantics.
- `tests/perf_gate.rs` with 4 regression budgets.
- `benches/guard.rs` with 6 criterion benchmark functions.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-auth-guard-v1.0.0...HEAD
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-auth-guard-v1.0.0
