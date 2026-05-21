# Changelog

All notable changes to `mailrs-inbound` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.2] - 2026-05-22

### Added
- New perf gate `pipeline_run_dispatch_overhead_under_budget`: measures `Pipeline::run` framework cost with four `NoopStage`s (async dispatch + final `make_delivery_decision` only, no real stage I/O). Budget 100 µs, observed ~3 µs.

### Changed
- `BUDGETS.md` clarifies that real-world `Pipeline::run` cost is owned by consumer stage backends; the dispatch gate guards framework-level regressions (per-stage alloc, mutex on hot path).

## [1.0.1] - 2026-05-22

### Added
- Initial perf regression gates and `BUDGETS.md`, closing the phase-5 polish gap.

## [1.0.0] - 2026-05-21

### Added
- Initial release. Composable SMTP receive pipeline framework: `Stage` trait, early-reject executor, pure decision logic, and RFC 8601 Authentication-Results helpers. Framework-only — consumers bring their own greylist, DKIM, virus-scan, and scoring stages.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-inbound-v1.0.2...HEAD
[1.0.2]: https://github.com/goliajp/mailrs/compare/mailrs-inbound-v1.0.1...mailrs-inbound-v1.0.2
[1.0.1]: https://github.com/goliajp/mailrs/compare/mailrs-inbound-v1.0.0...mailrs-inbound-v1.0.1
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-inbound-v1.0.0
