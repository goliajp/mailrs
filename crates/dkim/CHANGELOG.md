# Changelog

All notable changes to `mailrs-dkim` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.0] - 2026-05-23

### Added

- **`a=ed25519-sha256`** signature verification per RFC 8463.
  Closes the algorithm gap left in 1.0; the verifier now supports
  both real-world algorithms.
- `Algorithm::Ed25519Sha256` variant.
- `ed25519-dalek = "2"` dependency.
- 1 new test (`parse_ed25519_sha256_algorithm`).

RFC 8463 specifics:
- Public key in TXT is raw 32-byte Ed25519 key, base64-encoded
  (NOT PKCS8-wrapped like RSA).
- Signature is over the SHA-256 hash of the canonicalized
  signed-header block (the hash, not the block itself — RFC 8463 §3).

## [1.0.0] - 2026-05-23

### Added

- Initial release. DEPS_AUDIT #1 sibling to `mailrs-spf` 1.0.0.
- DKIM-Signature header parser supporting all RFC 6376 tags
  (v, a, b, bh, c, d, h, l, q, s, t, x, i, z).
- Canonicalization: simple + relaxed for both header and body, all 4
  combinations.
- Body hash (SHA-256) with optional `l=` length limit.
- Header hash over the signed-header list in document order with
  the DKIM-Signature value appended with `b=` cleared.
- Public-key TXT lookup via pluggable [`DkimResolver`] trait.
- Bundled `HickoryDkimResolver` behind default `hickory` feature.
- `a=rsa-sha256` signature verification via the `rsa` crate.
- Seven RFC 8601 result values via [`DkimResult`].
- 30 inline unit tests covering: header parser (all tag forms,
  defaults, error paths, canon combinations), canonicalization
  (simple body, relaxed body, simple header, relaxed header,
  unfolding, WSP normalization, length limit), verifier helpers
  (body offset detection, header value extraction, clear_b, PKCS8
  pubkey extract).

### Out of scope (1.0)

- `a=ed25519-sha256` (RFC 8463). Deferred to 1.1.
- Multiple `DKIM-Signature:` headers. First-match only in 1.0.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-dkim-v1.0.0...HEAD
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-dkim-v1.0.0
