# Changelog

All notable changes to `mailrs-intelligence` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [4.0.0] - 2026-10-22

### Changed

- **Breaking:** the `kevy-cache` feature now depends on `kevy-embedded`
  6 (was 4). `KevySpamCache::new` takes a `kevy_embedded::Store` from
  that version. Migration: depend on `kevy-embedded = "6"` and pass a
  store opened with it; the method signature is otherwise unchanged.

## [3.1.0] - 2026-08-04

### Added
- `assist` module: request/response types and prompt builders for
  writing assistance — `polish` / `polish_prompt` (`PolishRequest`,
  `PolishResult`), `reply_suggest` / `reply_suggest_prompt` /
  `parse_suggestions` (`ReplySuggestRequest`, `ReplySuggestResult`) and
  `generate_subject` / `subject_prompt` (`SubjectGenerateRequest`,
  `SubjectGenerateResult`), plus `AssistUnavailable` for the
  "no model configured" case. Input is sanitised before it reaches a
  prompt: unknown tones are rejected, language hints are validated and
  control characters are stripped (newlines kept).
- `importance::signals_for_inbound`, with the input structs
  `ContactFacts` and `MessageFacts`: builds `ImportanceSignals` for an
  inbound message, including the fallback that treats an unknown sender
  with bulk headers as a mailing list.

## [3.0.0] - 2026-06-03

### Changed (BREAKING)
- `KevySpamCache::new(conn: redis::aio::ConnectionManager)` →
  `KevySpamCache::new(store: kevy_embedded::Store)`. The cache now runs
  directly against the in-process kevy `Store` — no RESP wire, no
  network hop. `Store: Clone` so callers typically pass a clone of the
  shared store owned by the application.
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
