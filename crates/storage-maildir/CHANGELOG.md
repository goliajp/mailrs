# Changelog

All notable changes to `mailrs-maildir` are documented in this file.

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
- Criterion benchmarks for atomic delivery, directory scans, and flag parsing.

## [1.0.0] - 2026-05-19

### Added
- Initial release. Maildir filesystem-format primitives: atomic delivery, directory scans, and flag parsing. No IMAP or POP3 layer.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-maildir-v1.0.2...HEAD
[1.0.2]: https://github.com/goliajp/mailrs/compare/mailrs-maildir-v1.0.1...mailrs-maildir-v1.0.2
[1.0.1]: https://github.com/goliajp/mailrs/compare/mailrs-maildir-v1.0.0...mailrs-maildir-v1.0.1
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-maildir-v1.0.0
