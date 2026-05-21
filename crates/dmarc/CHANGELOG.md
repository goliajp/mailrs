# Changelog

All notable changes to `mailrs-dmarc` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.2] - 2026-05-22

### Added
- `DmarcResultRecord` now derives `Default` and exposes a fluent `pub fn new(...)` constructor. Both are additive; existing struct-literal call sites keep working.
- Rustdoc on `DmarcResultRecord` with a six-argument constructor example and a partial-construction example via `Default::default()`.

### Notes
- `#[non_exhaustive]` was deliberately not added — it would be a breaking change for external struct-literal users. A future 2.0 will tighten the type with `#[non_exhaustive]` plus a builder.

## [1.0.1] - 2026-05-21

### Added
- `#![deny(missing_docs)]` gate; all public items now carry rustdoc.
- Perf regression gates under `tests/perf_gate.rs` with documented budgets in `BUDGETS.md`.

### Changed
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.
- Refreshed dev dependencies (criterion 0.7 → 0.8).

## [1.0.0] - 2026-05-20

### Added
- Initial release. DMARC (RFC 7489) aggregate report generation: result recording, XML report builder, report-mail formatter, and `rua` extraction. Pluggable store trait with a PostgreSQL reference implementation.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-dmarc-v1.0.2...HEAD
[1.0.2]: https://github.com/goliajp/mailrs/compare/mailrs-dmarc-v1.0.1...mailrs-dmarc-v1.0.2
[1.0.1]: https://github.com/goliajp/mailrs/compare/mailrs-dmarc-v1.0.0...mailrs-dmarc-v1.0.1
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-dmarc-v1.0.0
