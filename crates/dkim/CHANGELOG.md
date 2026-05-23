# Changelog

All notable changes to `mailrs-dkim` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.3.0] - 2026-05-23

### Added

- **`verify_all(resolver, raw_message) -> Vec<SignatureOutput>`** —
  the multi-signature counterpart to `verify`. Walks every
  `DKIM-Signature` header on the message and verifies each one
  independently, returning one `SignatureOutput { result, header }`
  per signature.

  Real-world messages routinely carry 2-3 signatures: the original
  signer, every forwarder that re-signs, mailing-list software that
  attaches a list-signature. DMARC alignment must consider every
  `d=` independently (any aligned-and-passing signature satisfies the
  aligned-DKIM half of DMARC), so a single-signature `verify` left
  the caller to roll their own multi-sig walk. `verify_all` removes
  that hazard.

  `SignatureOutput::domain()` + `SignatureOutput::is_pass()` are the
  two-line API for DMARC consumers.

- `pub headers::find_all_header_values_in_raw(headers, name) -> Vec<String>`
  — the multi-match counterpart to `find_header_value_in_raw`.
  Returns folded values in source order.

### Changed

- `verify_inner` factored into `verify_one(resolver, raw, header_value,
  headers_raw, body_offset)` so `verify` and `verify_all` share the
  same per-signature pipeline. Behaviour identical; `verify` still
  walks the first `DKIM-Signature` and returns the same `DkimResult`
  values it did in 1.2.

### Tests

- `tests/multi_sig.rs` — end-to-end: two real RSA-2048 keypairs sign
  the same message under different selectors / domains, resolver maps
  each selector to its public key TXT, `verify_all` returns both
  with `Pass` + correct `d=`. Plus an "empty message → empty result"
  test and a "one good signature + one tampered signature →
  Pass + Fail" test.

### Impact on `mailrs` server

This release is the prerequisite for cutting server DMARC over to
`mailrs-dmarc` (DEPS_AUDIT #1 final step). DMARC alignment needs
per-signature `d=`, which requires `verify_all`.

## [1.2.0] - 2026-05-23

### Added

- `pub mod crypto` — extracted from `verifier.rs`:
  - `extract_public_key(txt)` — parse a DKIM TXT record's `p=` tag
    into raw key bytes (PKCS8 DER for RSA, raw 32 bytes for Ed25519).
  - `verify_signature(algorithm, key, signed_data, sig)` — standalone
    RSA-SHA256 / Ed25519-SHA256 verify primitive. No DKIM-Signature
    layout assumptions, so other email-auth crates (e.g. `mailrs-arc`'s
    AMS / AS verify) can reuse exactly the same crypto core.
- `pub mod headers` — extracted from `verifier.rs`:
  - `find_body_offset` / `body_offset_minus_blank` — RFC 5322
    headers-vs-body terminator location, tolerant of lone-LF EOL.
  - `find_header_value` / `find_header_value_in_raw` — RFC 5322
    fold-aware (`CRLF + WSP`) header lookup, owning + borrowing
    variants.
  - `clear_b_value` — replace the value of a `b=` tag in a signature
    header with empty bytes, for header-hash input construction.

### Changed

- `verifier::verify` now delegates to `crypto::verify_signature` and
  `headers::*` instead of inlining the implementations. Behaviour
  identical; the lift is purely about reuse. Verified by the existing
  47-test unit suite.

### Notes

This release exists to support the upcoming `mailrs-arc` 1.1 crypto
AMS / AS verify path. The new modules are deliberately small and the
function signatures are stable — they are the API contract
`mailrs-arc` 1.1 will depend on.

## [1.1.3] - 2026-05-23

### Changed

- DKIM-Signature parser dispatch swapped from a 13-arm
  `if name.eq_ignore_ascii_case("v") { ... } else if ...` chain to
  `match name.as_bytes() { b"v" => ..., b"a" => ..., ... }`. Lowercase
  byte-match is the hot path; mixed-case tag names fall through to a
  cold case-insensitive fallback (RFC 6376 §3.2 compatibility preserved).
- `h=` (signed-headers) parsing rewritten as a single byte-iteration
  with `String::from_utf8_unchecked` on the per-header buffer
  (`unsafe` justified — only ASCII-lowercased bytes ever pushed).
  Drops one `.chars()` + one `.to_ascii_lowercase()` allocation per
  signed header on a header that typically lists 5-10 names.

### Performance

Measured (criterion, M-series Mac, release, full-sample):

| Input | 1.1.1 | 1.1.2 (perf-batch) | 1.1.3 (this) | mail-auth 0.9 |
|---|---:|---:|---:|---:|
| minimal (7 tags) | 674 ns | 158 ns | **147 ns** | 167 ns |
| realistic (folded, 11 tags) | 1.4 µs | 436 ns | **405 ns** | 423 ns |

Cumulative: 4.6× / 3.5× speedups over the 1.1.1 baseline. We now
**beat `mail-auth`** on both inputs (+12% / +4%). The earlier 7%
gap on the realistic case came from `h=` per-element String
allocations, not from the dispatch.

## [1.1.2] - 2026-05-23

### Changed

- **DKIM-Signature header parser rewritten as a single-pass byte scanner.**
  Replaces the prior `unfold(...) + parse_tag_list(...) -> HashMap<String,String>`
  pipeline. No public API change; 44 inline tests unchanged + green.

### Performance

Measured (criterion, M-series Mac, release, `--quick`):

| Input | Before | After | mail-auth 0.9 |
|---|---:|---:|---:|
| minimal (7 tags) | 674 ns | **158 ns** (−77%) | 159 ns |
| realistic (folded, 11 tags) | 1.4 µs | **436 ns** (−69%) | 405 ns |

Result: from 4.1× / 3.6× slower than `mail-auth` → within ±7%. Bench source:
`benches/compare_mail_auth.rs`. Reproduce: `cargo bench -p mailrs-dkim --bench compare_mail_auth`.

### Added

- `benches/compare_mail_auth.rs` — head-to-head bench against the same `mail-auth`
  version the workspace already pulls in.

## [1.1.1] - 2026-05-23

### Added

- `tests/perf_gate.rs` with 5 regression budgets (parser + canon).
- `BUDGETS.md` documenting the perf table + non-budgets.

No lib code change.

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
