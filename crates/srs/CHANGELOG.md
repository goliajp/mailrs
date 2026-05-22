# Changelog

All notable changes to `mailrs-srs` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.1] - 2026-05-22

### Added

- 13 new edge-case tests: hash too short, hash too long, tt wrong
  length, too few `=` separators, lowercase prefix rejection, empty
  local-part rewrite, local-part containing `+`, tt-window edge
  with yesterday's timestamp, tt-window wrap-around math,
  constant-time-eq unequal length, long secrets (>HMAC key size),
  no-@ input rejection, RFC 6532 UTF-8 local-part roundtrip.
- Lib test count 15 → 28.

No behavior change; pure coverage-density bump.

## [1.0.0] - 2026-05-22

### Added

- Initial release. Extracted from `mailrs-server`'s smtp_session::srs
  module where it ran in production for ~1 year.
- `rewrite(sender, local_domain, secret)` — forward rewrite for
  SPF-aware mail forwarding, returns the SRS0= form.
- `reverse(rewritten, secret, window_days)` — parse SRS0= back to
  original sender with HMAC verify + timestamp-window check. Returns
  `Option<String>`; `None` for any failure (malformed, bad HMAC,
  expired).
- HMAC-SHA256 with 32-bit (8-hex-char) truncation. Sufficient for
  online-guess protection given the per-day timestamp.
- Constant-time HMAC byte comparison in `reverse()` to prevent
  timing-side-channel secret recovery.
- `DEFAULT_TIMESTAMP_WINDOW_DAYS = 14` constant for the typical
  bounce window.
- 14 inline unit tests covering: format shape, roundtrip recovery,
  tampered-hash rejection, wrong-secret rejection, malformed input,
  local-part with dots, subdomain in original.
- `tests/perf_gate.rs` with 4 regression budgets.
- `benches/srs.rs` with 4 criterion benchmark functions.

### Out of scope (deferred)

- SRS1 (forwarding-chain) — only needed for multi-hop forwarding where
  an already-rewritten SRS0= passes through a second forwarder. Most
  deployments don't see this; file an issue if you need it.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-srs-v1.0.0...HEAD
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-srs-v1.0.0
