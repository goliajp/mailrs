# Changelog

All notable changes to `mailrs-intelligence` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [3.0.0] - 2026-06-03

### Changed (BREAKING)
- `KevySpamCache::new(conn: redis::aio::ConnectionManager)` →
  `KevySpamCache::new(store: kevy_embedded::Store)`. The cache now runs
  directly against the in-process kevy `Store` — no RESP wire, no
  network hop. `Store: Clone` so callers typically pass a clone of the
  shared cement-owned store.
- Feature `kevy-cache` now pulls `kevy-embedded` instead of `redis`. No
  more `redis` transitive dependency.

### Migration
```toml
# before
mailrs-intelligence = "2"
redis = "1"
# after
mailrs-intelligence = "3"
kevy-embedded = "1.1"
```
```rust
// before
let cm = redis::aio::ConnectionManager::new(redis::Client::open("redis://…")?).await?;
let cache = KevySpamCache::new(cm);
// after
let store = kevy_embedded::Store::open(kevy_embedded::Config::default())?;
let cache = KevySpamCache::new(store);
```

## [2.0.0] - 2026-06-03

### Changed (BREAKING)
- Feature flag `redis-cache` renamed to `kevy-cache`. Update your `Cargo.toml`:
  `mailrs-intelligence = { version = "2", features = ["kevy-cache"] }`.
- Public struct `RedisSpamCache` renamed to `KevySpamCache`. Update imports:
  `use mailrs_intelligence::spam::KevySpamCache;`.
- Module `spam::redis_impl` renamed to `spam::kevy_impl` (re-exports still
  flat at `spam::KevySpamCache`).
- Documentation refers to kevy (<https://github.com/goliajp/kevy>); RESP
  wire protocol is unchanged so `redis://` URLs still work.

## [1.0.2] - 2026-05-22

### Added
- `#![deny(missing_docs)]` gate; all public items now carry rustdoc.
- Perf regression gates under `tests/perf_gate.rs` with documented budgets in `BUDGETS.md`.

### Changed
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.
- Refreshed dev dependencies (criterion 0.7 → 0.8).

## [1.0.1] - 2026-05-20

### Added
- Deeper unit-test coverage and criterion benches for extraction, importance scoring, and spam classification helpers.

## [1.0.0] - 2026-05-20

### Added
- Initial release. LLM-powered email analysis primitives: structured extraction, importance scoring, spam classification, and embeddings — with a pluggable `LlmProvider` trait and an OpenAI-compatible reference implementation.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-intelligence-v1.0.2...HEAD
[1.0.2]: https://github.com/goliajp/mailrs/compare/mailrs-intelligence-v1.0.1...mailrs-intelligence-v1.0.2
[1.0.1]: https://github.com/goliajp/mailrs/compare/mailrs-intelligence-v1.0.0...mailrs-intelligence-v1.0.1
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-intelligence-v1.0.0
