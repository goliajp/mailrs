# Changelog

All notable changes to `mailrs-rate-limit` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.1] - 2026-05-22

### Added
- New `benches/store.rs` (4 bench groups, 7 cases) covering pure math, sync/async hot keys, and stale cleanup.
- README `## Performance` section with measured criterion medians: `evaluate_bucket` ~1.7 ns, `check_sync` hot key ~33 ns, `check` async hot key ~84 ns, `cleanup_stale(10k)` ~100 µs. M-series Mac, release profile, 100-sample.

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

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-rate-limit-v1.0.1...HEAD
[1.0.1]: https://github.com/goliajp/mailrs/compare/mailrs-rate-limit-v1.0.0...mailrs-rate-limit-v1.0.1
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-rate-limit-v1.0.0
