# Changelog

All notable changes to `mailrs-jmap` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.2] - 2026-05-23

### Added

- 32 new inline tests in `methods/email.rs` (23) and `methods/submission.rs` (9)
  exercising `Email/get`, `Email/query`, `Email/set`, and
  `EmailSubmission/set` happy paths and edge cases:
  - `Email/get`: missing IDs, malformed IDs, mixed existing+missing,
    missing `ids` argument.
  - `Email/query`: `inMailbox` filter (matching and unknown), `hasKeyword`
    / `notKeyword` filters, case-insensitive `text` filter, ascending /
    descending sort, `position` pagination, `limit` clamping at 500.
  - `Email/set`: `destroy` sets `FLAG_DELETED`, malformed-id partition
    into `notDestroyed`, keyword replace + PatchObject path form
    (`keywords/$seen: true|false`), clearing one flag preserves others.
  - `EmailSubmission/set`: missing/malformed `emailId`, missing db row,
    `read_message_raw` returning `None`, success/failure outcome
    propagation, multiple-create partition.
- Lib test count: 36 → 68.

No lib code change.

## [1.1.1] - 2026-05-22

### Added
- README `## Performance` section with measured criterion medians: `keywords_to_flags` ~5.6 ns, dispatch `Email/query` ~2.4 µs, `dispatch_request` back-ref ~10.4 µs. M-series Mac, release profile, 100-sample.

## [1.1.0] - 2026-05-21

### Added
- Public in-memory `MailStore` fixture, promoted from `dev-dependencies` so downstream crates can reuse it in their own tests and examples.
- Perf regression gates under `tests/perf_gate.rs` covering dispatcher and request composition, with documented budgets in `BUDGETS.md`.

### Changed
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.

## [1.0.3] - 2026-05-21

### Added
- Criterion benchmarks for dispatcher and request composition under `benches/`.

## [1.0.2] - 2026-05-21

### Added
- Protocol-level integration coverage: end-to-end JMAP scenarios against the in-memory store.

## [1.0.1] - 2026-05-20

### Added
- `#![deny(missing_docs)]` gate; all 37 public items now carry rustdoc.
- Expanded README with quick-start example and trait overview.

## [1.0.0] - 2026-05-20

### Added
- Initial release. Framework-agnostic JMAP (RFC 8620 + RFC 8621) server-side dispatcher and method handlers, with the mail store pluggable via the `MailStore` trait.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-jmap-v1.1.1...HEAD
[1.1.1]: https://github.com/goliajp/mailrs/compare/mailrs-jmap-v1.1.0...mailrs-jmap-v1.1.1
[1.1.0]: https://github.com/goliajp/mailrs/compare/mailrs-jmap-v1.0.3...mailrs-jmap-v1.1.0
[1.0.3]: https://github.com/goliajp/mailrs/compare/mailrs-jmap-v1.0.2...mailrs-jmap-v1.0.3
[1.0.2]: https://github.com/goliajp/mailrs/compare/mailrs-jmap-v1.0.1...mailrs-jmap-v1.0.2
[1.0.1]: https://github.com/goliajp/mailrs/compare/mailrs-jmap-v1.0.0...mailrs-jmap-v1.0.1
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-jmap-v1.0.0
