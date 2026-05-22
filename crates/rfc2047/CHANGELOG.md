# Changelog

All notable changes to `mailrs-rfc2047` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.0] - 2026-05-22

### Added

- `encode(&str) -> Cow<'_, str>` — complement to `decode`. ASCII input
  returns borrowed (no allocation, no wrapping). Non-ASCII input
  becomes `=?UTF-8?B?<base64>?=`. Measured ~80 ns ASCII passthrough,
  ~130 ns Japanese encode (criterion, release).
- Roundtrip test: `decode(encode(s))` returns `s` for arbitrary UTF-8
  strings (verified for emoji, CJK, Latin-extended).
- Comparative bench `encode/japanese` extends `benches/decode.rs`.

## [1.0.0] - 2026-05-22

### Added

- Initial release. `decode(&[u8]) -> Cow<'_, str>` handles RFC 2047
  encoded-word tokens: `=?charset?(B|Q)?text?=`.
- Both base64 (B) and quoted-printable (Q) encodings supported.
- Charset → UTF-8 via `encoding_rs` (WHATWG Encoding spec): UTF-8,
  ISO-8859-*, Windows-*, ISO-2022-JP, Shift_JIS, EUC-JP, EUC-KR,
  Big5, GB18030, etc.
- RFC 2047 §6.2 "whitespace between adjacent encoded-words is dropped"
  handled.
- ASCII-only input is the fast path: returned as `Cow::Borrowed` with
  zero allocation.
- Malformed tokens degrade gracefully (no panic; literal `=?` is
  preserved).
- Companion to `mailrs-rfc5322`: pair them to get the decoded value
  of any header.
- `tests/perf_gate.rs` with 4 regression gates.
- `benches/decode.rs` with comparative numbers vs `mail-parser` 0.11.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-rfc2047-v1.0.0...HEAD
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-rfc2047-v1.0.0
