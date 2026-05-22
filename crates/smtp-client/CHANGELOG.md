# Changelog

All notable changes to `mailrs-smtp-client` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.3] - 2026-05-22

### Added
- README `## Performance` section with measured criterion medians: `parse_response(short)` ~30 ns, `sort_mx_records(20)` ~12 ns, `dot_stuff(5KB)` ~1.4 µs. M-series Mac, release profile, 100-sample.

## [1.0.2] - 2026-05-22

### Added
- `#![deny(missing_docs)]` gate; all public items now carry rustdoc.
- Perf regression gates under `tests/perf_gate.rs` with documented budgets in `BUDGETS.md`.

### Changed
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.
- Refreshed dev dependencies (criterion 0.7 → 0.8).

## [1.0.1] - 2026-05-20

### Added
- Criterion benchmarks for response parsing and MX resolution helpers.

## [1.0.0] - 2026-05-20

### Added
- Initial release. Async outbound SMTP client primitives: MX resolution, DANE/STARTTLS, and response parsing — transport-agnostic for RFC 5321 senders.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-smtp-client-v1.0.3...HEAD
[1.0.3]: https://github.com/goliajp/mailrs/compare/mailrs-smtp-client-v1.0.2...mailrs-smtp-client-v1.0.3
[1.0.2]: https://github.com/goliajp/mailrs/compare/mailrs-smtp-client-v1.0.1...mailrs-smtp-client-v1.0.2
[1.0.1]: https://github.com/goliajp/mailrs/compare/mailrs-smtp-client-v1.0.0...mailrs-smtp-client-v1.0.1
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-smtp-client-v1.0.0
