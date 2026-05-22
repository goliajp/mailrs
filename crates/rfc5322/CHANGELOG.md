# Changelog

All notable changes to `mailrs-rfc5322` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.1] - 2026-05-22

### Changed

- **24% faster header lookup on standard messages, 53% faster worst-case.**
  `Message::header` now runs a fast-path that checks the candidate
  name prefix + colon BEFORE calling `from_utf8` and the colon-finder
  on the full line. Non-matching headers are rejected with just a
  byte-level case-insensitive compare. Match path is unchanged.
  Measured: 277 ns → 212 ns (1 KB), 833 ns → 393 ns (target at end
  of 50 headers).
- 20 new edge-case tests: 3+ continuation-line folding, tab
  continuation, header value with colons (Received chain), empty
  value, whitespace-only value, header name case preservation,
  long-value (2 KB) DKIM-Signature handling, target-at-end of 50
  headers, missing-header rejection (no prefix-match leak), UTF-8 in
  header value via `header_str`, body with internal empty lines kept
  intact. Test count 23 → 43.

## [1.0.0] - 2026-05-22

### Added

- Initial release. Pull-based RFC 5322 message parser: lazy header
  lookup (`Message::header`, `Message::header_str`, `Message::header_all`),
  iteration (`Message::headers`), and body access (`Message::body`,
  `Message::body_offset`).
- RFC 5322 §3.2.2 line folding handled — continuation lines starting
  with whitespace stay inside one header value.
- Both `\r\n` and `\n` line endings accepted.
- Zero allocations on the parse side; every returned `&[u8]` /
  `&str` borrows from the input bytes.
- Zero runtime dependencies. Only `mail-parser` + `criterion` are
  pulled in for benches.
- Measured **8-25× faster than `mail-parser`** for the
  "read-a-few-headers + body" pattern (criterion, M-series Mac,
  release; see [BUDGETS.md](BUDGETS.md) for the full table).
- `tests/perf_gate.rs` with regression budgets for header lookup,
  body offset, and received-chain walk.
- Companion `benches/parse.rs` with the comparative numbers vs
  `mail-parser`.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-rfc5322-v1.0.0...HEAD
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-rfc5322-v1.0.0
