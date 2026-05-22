# Changelog

All notable changes to `mailrs-rate-limit` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0] - 2026-05-22

### Added
- Initial release. Token-bucket rate limiting trait
  (`RateLimitStore`) + in-memory reference implementation
  (`InMemoryRateLimitStore`). `&str` keys, unix-seconds time, async
  trait surface — transport-agnostic and no protocol coupling.
- Pure-math entry point (`evaluate_bucket`) exposed for backend
  authors who want to plug their own storage in without going
  through the trait.
- 28 unit tests, 8 trait-contract tests, 4 perf-gate tests.
  Documented latency budgets in `BUDGETS.md`.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-rate-limit-v1.0.0...HEAD
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-rate-limit-v1.0.0
