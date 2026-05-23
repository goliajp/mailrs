# Changelog

All notable changes to `mailrs-mime` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.1] - 2026-05-23

### Added

- `tests/perf_gate.rs` with 3 regression budgets (parse simple,
  parse multipart, find text/calendar).
- `BUDGETS.md` documenting the perf table + non-budgets.

No lib code change.

## [1.0.0] - 2026-05-23

### Added

- Initial release. DEPS_AUDIT #2 stone — replaces residual
  `mail-parser` usage where only the MIME tree shape matters.
- `parse(raw) -> Part` top-level entry.
- `Part` struct with `content_type`, `disposition`, `content_id`,
  `transfer_encoding`, decoded `body`, and recursive `children`.
- `Part::walk()` depth-first iterator.
- `Part::find_by_content_type(mime)` — exact-match lookup.
- `Part::body_text()` — charset-aware decode for `text/*` leaves
  via `encoding_rs` (UTF-8, ISO-2022-JP, Shift_JIS, …).
- `Part::attachments()` + `Part::attachment_filename()` —
  attachment iteration with Content-Disposition + Content-Type
  filename fallback.
- `ContentType::parse` / `Disposition::parse` with RFC 2231
  parameter decoding via `mailrs-rfc2231` (so
  `filename*=UTF-8''…` surfaces as plain UTF-8).
- `TransferEncoding` enum + decoders: `base64`,
  `quoted-printable`, 7bit / 8bit / binary, `Other(_)` catch-all.
- Boundary splitter handles nested multipart correctly, ignores
  the prologue / epilogue per RFC 2046 §5.1.1.
- 20 inline content_type tests, 17 decoder tests, 18 part tests
  = **55 total** covering parse paths, nested multipart, attachment
  detection, charset decoding (incl. ISO-2022-JP), base64 +
  quoted-printable edge cases (soft line break, lowercase hex,
  malformed escape), RFC 2231 filename decode, content-id angle
  bracket strip.

### Out of scope (1.0)

- RFC 2047 encoded-word decode for header field VALUES — separate
  stone (`mailrs-rfc2047`). Use it on `From:` / `Subject:` /
  display names after extracting via `mailrs-rfc5322`.
- Outbound MIME building — this crate is read-only.
- Mojibake / charset autodetection — we trust the on-wire charset.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-mime-v1.0.0...HEAD
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-mime-v1.0.0

