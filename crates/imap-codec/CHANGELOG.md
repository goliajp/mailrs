# Changelog

All notable changes to `mailrs-imap-codec` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.0] - 2026-10-22

### Added

- Line and literal limits: `ImapCodec::with_max_line` /
  `with_max_literal` and their getters, and the `TooLong` error a decode
  over either limit wraps. Defaults are `MAX_LINE` (64 KiB) and
  `MAX_LITERAL` (50 MiB).

### Changed

- `ImapCodec::new()` is bounded by those defaults. It held any line and
  any literal before, however large.

## [1.0.1] - 2026-08-04

### Changed

- Line-mode decoding finds the CRLF with `memchr` (about 10% faster on
  long lines). Behaviour is unchanged.
- New dependency: `memchr`.

## [1.0.0] - 2026-05-23

### Added

- Initial release. `ImapCodec`, a `tokio_util::codec` `Decoder` /
  `Encoder` for IMAP sessions that switches between line mode
  (CRLF-terminated frames) and literal mode (exactly N raw bytes).
- `ImapCodec::expect_literal(size)`: after parsing a `{N}` marker, the
  protocol layer tells the codec to read the next N bytes as a literal.
- `ImapInput::Line` (lossy UTF-8) and `ImapInput::LiteralData` frames.
