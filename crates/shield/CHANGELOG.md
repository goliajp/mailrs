# Changelog

All notable changes to `mailrs-shield` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.2] - 2026-05-22

### Added
- README `## Performance` section with measured criterion medians: `interpret_spamhaus` ~700 ps, `ptr_score_from_names` ~85 ns, `triplet_key` ~120 ns. M-series Mac, release profile, 100-sample.

## [1.0.1] - 2026-05-22

### Added
- `#![deny(missing_docs)]` gate; all public items now carry rustdoc.
- Perf regression gates under `tests/perf_gate.rs` with documented budgets in `BUDGETS.md`.

### Changed
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.
- Refreshed dev dependencies (criterion 0.7 → 0.8).

## [1.0.0] - 2026-05-20

### Added
- Initial release. SMTP server anti-spam primitives in three modules: DNS blocklist (DNSBL) queries, greylisting policy with an optional Redis store, and PTR / forward-confirmed reverse DNS (FCrDNS) checks.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-shield-v1.0.2...HEAD
[1.0.2]: https://github.com/goliajp/mailrs/compare/mailrs-shield-v1.0.1...mailrs-shield-v1.0.2
[1.0.1]: https://github.com/goliajp/mailrs/compare/mailrs-shield-v1.0.0...mailrs-shield-v1.0.1
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-shield-v1.0.0
