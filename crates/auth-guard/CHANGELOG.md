# Changelog

All notable changes to `mailrs-auth-guard` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
