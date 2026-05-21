# Changelog

All notable changes to `mailrs-clean` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.2] - 2026-05-22

### Added
- Perf regression gates under `tests/perf_gate.rs` with documented budgets in `BUDGETS.md`.
- `#![deny(missing_docs)]` gate; all public items now carry rustdoc.

### Changed
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.
- Refreshed dev dependencies (criterion 0.7 → 0.8).

## [1.0.1] - 2026-05-20

### Fixed
- Corrected the `repository` URL typo in `Cargo.toml` so crates.io links resolve.

## [1.0.0] - 2026-05-20

### Added
- Initial release. Zero-I/O email content cleanup primitives: HTML sanitization, tracking-pixel detection, bulk/automated-sender heuristics, and quoted-reply splitting.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-clean-v1.0.2...HEAD
[1.0.2]: https://github.com/goliajp/mailrs/compare/mailrs-clean-v1.0.1...mailrs-clean-v1.0.2
[1.0.1]: https://github.com/goliajp/mailrs/compare/mailrs-clean-v1.0.0...mailrs-clean-v1.0.1
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-clean-v1.0.0
