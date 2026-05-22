# Changelog

All notable changes to `mailrs-rfc5322` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
