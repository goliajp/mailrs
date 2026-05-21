# Changelog

All notable changes to `mailrs-imap-proto` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.2] - 2026-05-22

### Added
- `#![deny(missing_docs)]` gate; all public items now carry rustdoc.
- Perf regression gates under `tests/perf_gate.rs` with documented budgets in `BUDGETS.md`.

### Changed
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.
- Refreshed dev dependencies (criterion 0.7 → 0.8).

## [1.0.1] - 2026-05-20

### Added
- Criterion benchmarks for command parsing and response formatting hot paths.

## [1.0.0] - 2026-05-19

### Added
- Initial release. Zero-I/O IMAP4rev1 protocol parser, response formatter, and sequence-set helpers covering RFC 3501 plus the NAMESPACE, SORT, ENABLE (RFC 5161), and UNSELECT (RFC 3691) extensions.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-imap-proto-v1.0.2...HEAD
[1.0.2]: https://github.com/goliajp/mailrs/compare/mailrs-imap-proto-v1.0.1...mailrs-imap-proto-v1.0.2
[1.0.1]: https://github.com/goliajp/mailrs/compare/mailrs-imap-proto-v1.0.0...mailrs-imap-proto-v1.0.1
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-imap-proto-v1.0.0
