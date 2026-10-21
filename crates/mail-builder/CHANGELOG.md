# Changelog

All notable changes to `mailrs-mail-builder` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- `LICENSE-APACHE` and `LICENSE-MIT` are now shipped with the crate.

## [1.0.1] - 2026-08-04

### Added

- `PartBytes` is re-exported at the crate root, so callers can drive
  `multipart_envelope` directly.

### Changed

- The header-terminator scan in `lint` and the boundary collision
  check now use `memchr` / `memmem` instead of byte-window walks.
  Output is unchanged; building a message with a 16 KiB attachment is
  about twice as fast, and the multipart envelope about five times.
- New dependency: `memchr` 2.

## [1.0.0] - 2026-05-29

### Added

- `MessageBuilder::report_type(...)`: `multipart/report` support for
  RFC 6522 / 3464 / 7489 (DSNs, DMARC aggregate reports).
- `MessageBuilder::build_strict()` and `lint(raw)`, an invariant checker
  for the produced bytes.

### Changed

- Attachment Content-Transfer-Encoding selection: `text/*` and
  `message/*` parts go through `choose_cte` (so a
  `message/delivery-status` body inside a DSN ships as 7bit ASCII rather
  than base64); all other attachments use base64.
- Validated against an RFC corpus, a 1000-sample differential parse and
  delivery through Mailpit. The API is considered stable from this
  release.

## [0.1.0]

Not published to crates.io.

### Added

- Initial version.
- `MessageBuilder` with chain-style setters for the standard
  RFC 5322 headers and 0-N attachments.
- Encoded-word (RFC 2047) auto-applied to non-ASCII header values
  via `mailrs-rfc2047`.
- Header folding per RFC 5322 §2.2.3 (78-char soft wrap).
- CTE auto-selection: 7bit / quoted-printable / base64 based on
  byte composition + max line length.
- `multipart/alternative` (text + html) and `multipart/mixed`
  (body + attachments).
- Multipart boundary collision-scan (regenerate if body matches).
- Output: raw `Vec<u8>` via `build()` or UTF-8 via `Display`.
