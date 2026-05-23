# Changelog

All notable changes to `mailrs-arc` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.2.0] - 2026-05-23

### Added

- **`pub mod seal`** — the outbound forwarder side. With 1.0 we
  parsed chains, with 1.1 we verified them; with 1.2 we can also
  produce them. mailrs-arc now covers both directions of RFC 8617.
  This is the first standalone Rust ARC implementation with
  sign+verify in a single independent crate (mail-auth bundles the
  whole email-auth stack; this is the carved-out primitive).
- `ArcSigningKey<'a>::{Rsa(&RsaPrivateKey), Ed25519(&SigningKey)}` —
  algorithm implied by the key variant. Borrows the parsed key so
  forwarders can share one parsed key across many seal calls.
- `SealOpts { domain, selector, signed_headers, canon_*, cv,
  authres, timestamp }` — everything a forwarder needs to attach
  one hop's worth of ARC headers.
- `seal(raw_msg, &key, &opts, prior) -> Result<SealedHeaders, ArcError>` —
  produces the three header lines (`aar`, `ams`, `seal`) ready to
  prepend in that order. `SealedHeaders::concat()` is the
  convenience helper.

### Validation rules enforced by `seal`

- First hop (`prior=None`): `cv` MUST be `None`.
- Later hop (`prior=Some(_)`): `cv` MUST NOT be `None`.
- New instance number (`prior.highest_instance()+1`) MUST be ≤ 50
  per RFC 8617 §4.2.1; otherwise `ArcError::ChainTooLong`.

### Tests

- `tests/seal_roundtrip.rs` — for every supported scenario:
  - First-hop seal: `cv=none` chain, run through
    `verify_chain_with_crypto` → `Pass`.
  - Two-hop seal: start with a key_A-signed first hop, then attach
    a key_B-signed second hop with `cv=pass`. Resolver returns the
    right key per query. Full chain → `Pass`.
  - First hop with `cv=pass` rejected (`InvalidCv`).
  - Later hop with `cv=none` rejected (`InvalidCv`).
- Plus 6 internal seal-side unit tests for type / string helpers.
- Full 50-test arc suite green (40 lib + 2 crypto roundtrip + 4
  perf gate + 4 seal roundtrip).

### Dependencies

- Bumps `mailrs-dkim` floor from `1.2` to `1.5` (needs the new
  `crypto::sign_signature` primitive).
- Adds direct `rsa` + `ed25519-dalek` runtime deps because the
  key types appear in the public `ArcSigningKey<'_>` enum.
  Already in the transitive graph via `mailrs-dkim`; making the
  deps direct keeps `cargo doc` resolving the types in this
  crate's docs.

### Why this matters

A verifier-only ARC crate is incomplete: an MTA that receives
forwarded mail can validate inbound chains but can't extend them
when it forwards further. `mailrs-arc` 1.2 closes that loop. The
first hop / later hop / `cv=` integrity rules are enforced in the
public API so callers can't produce malformed chains by accident.

## [1.1.0] - 2026-05-23

### Added

- **`crypto` module** — real cryptographic AMS and AS verification.
  Closes the gap reserved in 1.0's `ChainOutcome::CryptoUnimplemented`
  branch. Re-uses `mailrs_dkim::crypto::verify_signature` (lifted
  in dkim 1.2) and `mailrs_dkim::canon::*` for byte-identical
  canonicalization — no algorithm code is duplicated.
  - `verify_ams(&ArcSet, raw_message, &resolver)` — verifies one
    instance's `ARC-Message-Signature` body-hash + signed-header
    block against the DNS-fetched public key. Same canon + algorithm
    set as DKIM (`rsa-sha256` / `ed25519-sha256`).
  - `verify_as(&ArcChain, instance, &resolver)` — verifies one
    instance's `ARC-Seal` against the chain prefix (AAR_j + AMS_j +
    AS_j for `j=1..i`, then `AS_i` with `b=` cleared), always
    relaxed/relaxed per RFC 8617 §5.1.2.
- `verify_chain_with_crypto` now walks the chain from highest
  instance down and runs both `verify_ams` and `verify_as` for every
  set. Returns `ChainOutcome::Pass` only if every signature
  cryptographically validates; `ChainOutcome::Fail { reason }` names
  the first failure with `"ams i=N: …"` / `"as i=N: …"`.

### Tests

- `tests/crypto_roundtrip.rs` — full end-to-end RSA-2048 keypair →
  sign AMS over a real message → sign AS over the chain prefix →
  run `verify_chain_with_crypto` with a public-key-returning
  `DkimResolver` → assert `Pass`. Plus a tampered-body twin test
  that asserts `Fail` for body-hash mismatch.
- Existing 33 structural tests continue to pass.

### Errors added

- `ArcError::BodyHashMismatch` — body's recomputed SHA-256 didn't
  match `bh=`.
- `ArcError::InvalidBase64(tag)` — `b=` / `bh=` failed to decode.
- `ArcError::MalformedMessage` — no end-of-headers found in input.

### Deprecated

- `ChainOutcome::CryptoUnimplemented` — kept for API compatibility
  with 1.0 callers that pattern-matched on it, but
  `verify_chain_with_crypto` no longer returns this variant. New
  code should treat it as unreachable.

### Dependencies

- Bumps `mailrs-dkim` floor from `1.1` to `1.2` (needs the new
  `crypto` + `headers` modules).
- Dropped direct `rsa` + `ed25519-dalek` + `sha2` + `base64` deps
  from the runtime build — those now come transitively through
  `mailrs-dkim`. The crate's own compiled binary footprint is
  unchanged.
- `[dev-dependencies]` add `rsa` / `rand` / `base64` / `sha2` /
  `async-trait` for the crypto roundtrip integration tests only.

### Impact on `mailrs` server

This release lets the server drop its `mail_authenticator.verify_arc`
call (and ultimately `mail-auth` from its runtime deps once the
remaining DKIM / SPF shadow paths are removed). Tracked under
DEPS_AUDIT #1.

## [1.0.0] - 2026-05-23

### Added

- **`mailrs-arc` 1.0 first release.** RFC 8617 Authenticated Received
  Chain (ARC) header parsing, chain extraction, and structural
  verification. Fills a real Rust-ecosystem gap — until now the only
  ARC implementation was buried inside `mail-auth`'s ~5K-LOC umbrella.

- `header::ArcAuthResults::parse` — `ARC-Authentication-Results`
  parser. Pulls the mandatory `i=N` instance off and keeps the rest
  of the authres body verbatim for downstream walkers.

- `header::ArcMessageSignature::parse` — `ARC-Message-Signature`
  parser. Shares the DKIM-Signature tag-list shape; supports the
  full set of tags (i, a, b, bh, c, d, s, h, t, x) with the same
  byte-match dispatch + WSP-stripping for b/bh + lowercase signed-
  header list.

- `header::ArcSeal::parse` — `ARC-Seal` parser. Smaller tag set
  (i, a, b, cv, d, s, t); does NOT carry `h=` or `bh=` because the
  seal signs the chain (preceding ARC headers), not the body.

- `chain::ArcChain::extract(raw_message)` — walks the header block,
  groups the three header types by their `i=N` instance, and returns
  a `Vec<ArcSet>` in ascending instance order. Rejects sparse chains
  (`NonContiguousChain { missing }`) and chains with incomplete sets
  (`IncompleteSet { instance, missing }`). Header unfold is handled
  inline (RFC 5322 §2.2.3 continuation lines).

- `verify::verify_chain(&ArcChain)` — structural verification:
  contiguity, length ≤ 50 (RFC 8617 §4.2.1), `cv=` integrity
  (`i=1` must be `cv=none`, all later sets must be `cv=pass` or
  `cv=fail`). Returns `ChainOutcome::Pass` / `Fail { reason }`.

- `verify::verify_chain_with_crypto(chain, resolver, raw)` — async
  entry point that runs the structural layer and returns
  `ChainOutcome::CryptoUnimplemented` for 1.0. 1.1 will fill in the
  AMS / AS hash + RSA-SHA256 / Ed25519-SHA256 verify using
  [`mailrs_dkim::canon`] for byte-identical canonicalization.

- `resolver::ArcResolver` — type alias for `mailrs_dkim::DkimResolver`.
  ARC keys live at the same DNS shape as DKIM keys; one resolver
  feeds both verifiers.

- 33 inline tests covering: AAR/AMS/AS parse happy paths + every
  rejection (missing-tag, bad algorithm, empty h=, invalid cv=,
  instance 0 or > 50, malformed i=), chain extract (no-ARC →
  `Ok(None)`, single set, two-hop, header-order independence,
  incomplete-set rejection, non-contiguous rejection, folded-header
  unfold), structural verify (cv=none first-only rule, later-set
  cv=none rejection).

### Performance

Measured (criterion, M-series Mac, release):

| Operation | Median |
|---|---:|
| `ArcAuthResults::parse` | 21 ns |
| `ArcMessageSignature::parse` (realistic) | 479 ns |
| `ArcSeal::parse` (realistic) | 295 ns |
| `ArcChain::extract` (2-hop) | 3.65 µs |

### Dependencies

- `mailrs-dkim = "1.1"` — re-uses `DkimResolver` (same DNS shape as DKIM)
  and reserves canonicalization + RSA verify for the 1.1 crypto layer.
- `async-trait = "0.1"`, `base64 = "0.22"`, `sha2 = "0.10"`,
  `rsa = "0.9"`, `ed25519-dalek = "2"` — same crypto dependencies as
  `mailrs-dkim` (carried for 1.1; structurally unused in 1.0).

### Roadmap

- **1.1.0** — Cryptographic AMS + AS verify, plus ARC sealing
  (adding a new set on outbound forward). Closes
  [DEPS_AUDIT](https://github.com/goliajp/mailrs/blob/main/DEPS_AUDIT.md)
  candidate #1 — the server can drop `mail-auth` from its runtime
  deps once this lands.

[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-arc-v1.0.0
