# Changelog

All notable changes to `mailrs-mime` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.0.3] - 2026-10-22

### Fixed

- `Part::body_text` decodes through `mailrs_rfc2047::decode_charset`, so an
  ISO-2022-JP body that switches charset twice in a row no longer shows
  U+FFFD there.

### Changed

- Requires `mailrs-rfc2047` 1.2.

## [2.0.2] - 2026-08-04

### Fixed

- RFC 2231 parameter continuations (`filename*0*`, `filename*1*`, ...)
  are reassembled in index order and decoded once, so `filename()`
  finds names split across segments.
- RFC 2047 encoded-words inside a parameter value (as in
  `name="=?UTF-8?B?...?= =?UTF-8?Q?=2Epdf?="`, which RFC 2047 §5 forbids
  but senders write) are decoded. A value that only contains a literal
  `=?` is left alone.

## [2.0.1] - 2026-05-29

2.0.0 was not published to crates.io; its changes are included here.

### Changed

- **Breaking:** `Part` borrows from the input: `Part<'a>` with
  `body: Cow<'a, [u8]>` and `children: Vec<Part<'a>>`. `parse(raw)`
  returns `Part<'_>`, and `Walker` gains a lifetime (`Walker<'p, 'a>`).
- **Breaking:** `ContentType::type_` / `subtype` and `Disposition::kind`
  are `CompactString` instead of `String`.
- **Breaking:** `TransferEncoding::decode` returns `Cow<'_, [u8]>` and
  borrows the input when no decoding is needed.
- Faster parsing: a single pass collects MIME headers, the multipart
  boundary scan uses `memchr`, and clean base64 payloads skip an
  intermediate allocation.
- New dependencies: `memchr` 2, `compact_str` 0.9.

## [1.0.3] - 2026-05-23

### Changed

- **`parse` no longer allocates the multipart preamble** — was
  `body.to_vec()` on every multipart node (often 1KB+ per typical
  message), now `Vec::new()` since RFC 2046 §5.1.1 calls preamble
  "rarely interesting" and downstream code never reads it.
- **Header values borrow from the input** when ASCII (the common
  case). Previously `String::from_utf8_lossy` allocated a `String`
  for each of the 4 headers (Content-Type, Content-Disposition,
  Content-ID, Content-Transfer-Encoding) on every part. Now `&str`
  borrows in the hot path; lossy fallback only for non-UTF-8.
- **`split_multipart` builds delimiter bytes flat** instead of
  `format!("--{boundary}")`. Cheaper string concat for what amounts
  to a fixed-template byte buffer.

### Added

- `fuzz/` — libFuzzer target `parse` (committed in the previous
  fuzz-sweep commit). 2.5M iterations clean.

Public API unchanged; tests unchanged + green (45 lib tests).

## [1.0.2] - 2026-05-23

### Changed

- `find_by_content_type` rewritten: splits `mime_type` arg once into
  `(type, subtype)` and compares bytewise per part instead of calling
  `self.content_type.mime_type()` (which allocated `format!("{type}/{subtype}")`).
  Switched from `Walker` (Vec-backed) to recursive descent — no walk-stack
  allocation on the find path.

### Performance

Apples-to-apples comparison vs `mail-parser::is_content_type("text", "calendar")`
(criterion, M-series Mac, release, `--quick`):

| Input | mailrs-mime 1.0.1 | mailrs-mime 1.0.2 | mail-parser |
|---|---:|---:|---:|
| parse + find_calendar | 1.20 µs | 1.32 µs | 820 ns |

Honest: parse path still loses by ~60% (mail-parser has years of MIME-
specific optimization). `find_by_content_type` itself is now allocation-
free — but parse dominates the total cost.

### Added

- `bench-harness/` cross-language harness uses this crate for the Rust
  side of MIME comparisons.

No public API change.


## [1.0.1] - 2026-05-23

### Added

- `tests/perf_gate.rs` with 3 regression budgets (parse simple,
  parse multipart, find text/calendar).
- `BUDGETS.md` documenting the perf table + non-budgets.

No lib code change.

## [1.0.0] - 2026-05-23

### Added

- Initial release. Replaces residual
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
  crate (`mailrs-rfc2047`). Use it on `From:` / `Subject:` /
  display names after extracting via `mailrs-rfc5322`.
- Outbound MIME building — this crate is read-only.
- Mojibake / charset autodetection — we trust the on-wire charset.
