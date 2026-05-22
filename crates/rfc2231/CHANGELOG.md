# Changelog

All notable changes to `mailrs-rfc2231` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0] - 2026-05-22

### Added

- Initial release. Extracted from `mailrs-server`'s message_util
  module (where the `rfc2231_encode_param` half ran in production for
  ~1 year) and extended with the missing decoder.
- `encode_param(name, value)` — UTF-8 → wire. Emits `name="value"` for
  pure ASCII (legacy RFC 2045 form) or `name*=UTF-8''<pct>` for
  non-ASCII (RFC 2231 extended form). UPPERCASE hex throughout.
- `decode_param_value(s)` — wire → UTF-8. Accepts three real-world
  shapes: legacy quoted (`"value"`), legacy unquoted bareword
  (`value`), and RFC 2231 extended (`charset'lang'percent-encoded`).
  Returns `Cow::Borrowed` when no decode work was needed.
- Lenient percent-decode: `%X<non-hex>` and lone `%` are passed
  through as literal bytes (no rejection on malformed input).
- Charset → UTF-8 via `encoding_rs::for_label`.
- 22 inline unit tests covering: legacy quoted + bareword + extended
  forms, Japanese + Latin-1 charsets, language tag handling, unknown
  charset rejection, percent-decode lone-% / invalid-hex / lowercase
  hex tolerance, encode/decode roundtrip for ASCII + Japanese,
  backslash-escapes inside quoted strings, empty quoted string,
  apostrophe-in-quoted-not-treated-as-extended.
- `tests/perf_gate.rs` with 4 regression budgets.
- `benches/params.rs` with 7 criterion benchmark functions.

### Out of scope (deferred)

- RFC 2231 §3 continuation parameters (`filename*0=…; filename*1=…`).
  Rare in real-world mail; can be added in 1.x without compat break
  if needed.
- Full MIME-header line parsing (splitting `Content-Type: text/plain;
  charset=utf-8; foo=bar` into `(name, value)` pairs). Out of scope
  per "single focused concept" rule — pair with mailrs-rfc5322 or
  another header parser.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-rfc2231-v1.0.0...HEAD
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-rfc2231-v1.0.0
