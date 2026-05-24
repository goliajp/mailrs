# Changelog

All notable changes to `mailrs-arf` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-05-25

### Added

- Initial release. RFC 5965 Abuse Reporting Format (ARF) parser.
- `parse(&[u8]) -> Option<Report>` extracts every standard field
  from a `multipart/report; report-type=feedback-report` message.
- `Report::complainant()` convenience for the most-actionable single
  fact (recipient who triggered the report, with fallback to
  `Original-Mail-From`).
- 17 unit tests covering Hotmail-style + edge cases (lowercase
  normalization, address bracket stripping, header continuation
  unfolding, duplicate-header first-wins, non-UTF-8 graceful fall-
  through, default `feedback_type=abuse`).
- Criterion bench (`benches/arf.rs`) on Hotmail FBL sample +
  non-ARF early-exit path.
- `tests/perf_gate.rs` regression budgets (30 µs parse, 5 µs early
  exit; release < 2 µs / < 200 ns).
- `#![deny(missing_docs)]` gate, zero dependencies.

### Context

Carved out from `crates/server/src/fbl.rs` during Server Refactor v2
checkpoint v0.2 (cement 2nd-pass audit). The original 37-LOC
in-server helper was rebuilt with a full `Report` struct shape, all
11 RFC 5965 §3.2 fields, header-folding correctness, and lowercase
normalization for downstream suppression-list lookup keys.

First Rust ARF parser on crates.io as of release (existing `arf` crate
is an empty placeholder for an unrelated project; no functional
collision).
