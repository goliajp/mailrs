# Changelog

All notable changes to `mailrs-webhook-signature` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0] - 2026-05-22

### Added

- Initial release. Extracted from `mailrs-server`'s
  `webhook::signer` module (~75 LOC, ran in production for ~1 year)
  and extended with header-format helpers + rotation support.
- `sign(secret, payload)` — HMAC-SHA256, returns 64-char lowercase
  hex.
- `verify(secret, payload, signature)` — constant-time HMAC compare
  via `hmac::Mac::verify_slice`; returns `false` for any malformed
  hex input without panicking.
- `verify_header(secret, payload, header_value)` — sugar over
  [`parse_header`] + [`verify`]; accepts both `sha256=<hex>` and
  bare hex.
- `verify_any(&[secret1, secret2, …], payload, signature)` — supports
  secret rotation: returns `true` if ANY listed secret verifies.
- `format_header(signature)` — emits `sha256=<hex>` canonical form.
- `parse_header(header_value)` — strips `sha256=` prefix + trims
  whitespace; bare hex passes through.
- 21 inline unit tests covering: determinism, hex output shape,
  correct/wrong secret + tampered payload, empty payload + empty
  secret, 100 KB payload, long (1 KB) secret, invalid hex (no panic),
  format_header empty + nonempty, parse_header all 3 forms (prefix,
  bare, whitespace-padded), verify_any first/second/none match.
- `tests/perf_gate.rs` with 4 regression budgets.
- `benches/signing.rs` with 9 criterion benchmark functions.

### Out of scope (deferred)

- Algorithm negotiation beyond SHA-256. The vast majority of webhook
  APIs in production use exactly HMAC-SHA256; if you need SHA-512 /
  BLAKE3, write a parallel crate.
- Timestamp tolerance / replay protection. Embed your own timestamp
  inside the payload before signing if needed.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-webhook-signature-v1.0.0...HEAD
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-webhook-signature-v1.0.0
