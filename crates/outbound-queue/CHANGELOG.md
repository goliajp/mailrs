# Changelog

All notable changes to `mailrs-outbound-queue` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
