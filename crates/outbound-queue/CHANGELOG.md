# Changelog

All notable changes to `mailrs-outbound-queue` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.0] - 2026-05-23

### Changed

- **`retry::retry_delay_secs` now backed by `mailrs-backoff`** —
  internal switch from the hardcoded 8-slot array to
  `Backoff::smtp_outbound` (initial 60s, 2.5× growth, 8h cap).
  Schedule curve shifts slightly in attempts 1-5; cap unchanged at
  attempt 7+.

  Old: `[60, 300, 900, 1800, 3600, 7200, 14400, 28800]`
  New: `[60, 150, 375, 937, 2343, 5859, 14648, 28800]`

  Tests that asserted exact pre-1.1 values updated. If your
  deployment requires the exact old curve, pin to 1.0.2 or compute
  your own delays from a custom `mailrs_backoff::Backoff`.

### Added

- `retry::retry_delay_secs_jittered(attempt, seed)` — same curve with
  Full jitter applied. Use this in production schedulers to spread
  retry traffic and avoid synchronized retry bursts when many queue
  rows fail simultaneously (MX outage, DNS hiccup).
- `mailrs-backoff` runtime dep.

## [1.0.2] - 2026-05-22

### Added
- `#![deny(missing_docs)]` gate; all public items now carry rustdoc.
- Perf regression gates under `tests/perf_gate.rs` with documented budgets in `BUDGETS.md`.

### Changed
- Moved `mod tests` to the end of the file so production code no longer follows the test module; removed the `#[allow(clippy::items_after_test_module)]` workaround.
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.
- Refreshed dev dependencies (criterion 0.7 → 0.8).

## [1.0.1] - 2026-05-20

### Added
- Deeper unit-test coverage and criterion benches for DKIM signing and DSN-formatting helpers.

## [1.0.0] - 2026-05-20

### Added
- Initial release. Outbound mail queue primitives: DKIM signing, DSN generation, MTA-STS lookup, DANE TLSA verification, scheduled send, undo send, bounce processing with suppression list, and retry/backoff — with a pluggable store trait and a PostgreSQL reference implementation.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-outbound-queue-v1.0.2...HEAD
[1.0.2]: https://github.com/goliajp/mailrs/compare/mailrs-outbound-queue-v1.0.1...mailrs-outbound-queue-v1.0.2
[1.0.1]: https://github.com/goliajp/mailrs/compare/mailrs-outbound-queue-v1.0.0...mailrs-outbound-queue-v1.0.1
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-outbound-queue-v1.0.0
