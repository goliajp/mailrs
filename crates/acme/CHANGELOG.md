# Changelog

All notable changes to `mailrs-acme` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0] - 2026-05-23

### Added

- Initial release. Extracted from `mailrs-server`'s `acme.rs` module
  (~310 LOC, ran in production for ~1 year) when re-auditing for
  misclassified cement under the project's aggressive stone lens.
- `init(email, domains, acme_dir, staging, tokens)` — bootstrap path
  that loads existing valid cert or provisions a new one via HTTP-01.
- `load_or_create_account(email, staging, acme_dir)` — account flow
  separated for callers wanting more control.
- `provision_cert(account, domains, tokens)` — pure provision step
  via HTTP-01 challenge.
- `cert_days_remaining(pem_bytes)` — x509-parser-backed expiry helper.
- `save_cert(acme_dir, cert, key)` + `build_server_config(cert, key)`
  — filesystem + rustls glue.
- `spawn_renewal_task(account, tokens, tls_state, config, shutdown)`
  — periodic check + auto-swap into `mailrs-tls-reload::TlsState`.
- `RenewalConfig` (default: 12h interval, renew at ≤30 days).
- `spawn_challenge_server(tokens, addr, shutdown)` — bundled axum
  HTTP-01 server (feature-gated `axum-http`, on by default).
- `ChallengeTokens` type + `new_challenge_tokens()` constructor.
- 7 inline tests covering: empty store, insert/read, RenewalConfig
  default, Clone, garbage-PEM rejection, empty-input rejection,
  save_cert creates dir, save_cert overwrites.
- Compared to the in-server version: replaces `eprintln!` with
  `tracing` calls, makes the renewal interval + threshold configurable,
  makes the challenge port configurable (was hardcoded :80).

### Out of scope (deferred)

- DNS-01 challenge (provider-specific; future sibling crate).
- TLS-ALPN-01 challenge (rare in practice; sibling crate if demand).
- EAB-protected CA (instant-acme supports it; expose if needed).
- File watcher for external renewals (one `tls_state.swap` call from
  your own watcher).

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-acme-v1.0.0...HEAD
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-acme-v1.0.0
