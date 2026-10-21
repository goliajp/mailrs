# Changelog

All notable changes to `mailrs-smtp-client` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `SmtpConnection::auth_plain(user, secret)` and
  `SmtpConnection::auth_xoauth2(user, token)` for submission, with the
  payload builders `auth_plain_payload` and `auth_xoauth2_payload`. Both
  refuse to send credentials over a plaintext connection. A rejected
  XOAUTH2 token (`334` plus an error object) is reported as a failure
  instead of hanging the connection.
- `pkix_root_store()`: the trust anchors peers are verified against.

### Changed

- PKIX verification (`default_pkix_client_config` and the PKIX part of
  DANE usages 0 and 1) trusts the platform certificate store
  (`rustls-native-certs`, new dependency) plus the bundled
  `webpki-roots` set, plus a compiled-in `DigiCert Global Root CA`.
  Previously only `webpki-roots` was used, which no longer contains that
  root, so delivery to Microsoft 365 (`*.mail.protection.outlook.com`)
  failed with `UnknownIssuer`.

## [2.0.1] - 2026-05-29

2.0.0 was not published.

### Changed (BREAKING)

- `SmtpResponse::lines` is `Vec<CompactString>` (from `compact_str`, new
  dependency) instead of `Vec<String>`, which removes most allocations
  when parsing EHLO responses.

### Added

- `SmtpConnection::try_starttls_with_config(hostname, ClientConfig)` to
  STARTTLS with a caller-supplied rustls configuration (pinned
  certificates, DANE-only trust, test verifiers).
- `default_pkix_client_config()`, the configuration `try_starttls` uses.

## [1.1.0] - 2026-05-23

### Added

- `SmtpConnection::try_starttls` and `try_starttls_dane`, returning a
  `StarttlsResult`: `Success(conn)`, `Rejected { conn, code, message }`
  (the plaintext connection is handed back) or
  `HandshakeFailed { outcome, source }`.
- `TlsOutcome`, a classification of handshake failures (expired,
  host mismatch, untrusted, DANE failure, invalid name, network error,
  …) aligned with TLS-RPT (RFC 8460) result types, with `as_str` and
  `detail`.

### Changed

- `starttls` and `starttls_dane` are now thin wrappers over the `try_*`
  variants; their signatures are unchanged.

## [1.0.3] - 2026-05-22

### Added
- README `## Performance` section with measured criterion medians: `parse_response(short)` ~30 ns, `sort_mx_records(20)` ~12 ns, `dot_stuff(5KB)` ~1.4 µs. M-series Mac, release profile, 100-sample.

## [1.0.2] - 2026-05-22

### Added
- `#![deny(missing_docs)]` gate; all public items now carry rustdoc.
- Perf regression gates under `tests/perf_gate.rs` with documented budgets in `BUDGETS.md`.

### Changed
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.
- Refreshed dev dependencies (criterion 0.7 → 0.8).

## [1.0.1] - 2026-05-20

### Added
- Criterion benchmarks for response parsing and MX resolution helpers.

## [1.0.0] - 2026-05-20

### Added
- Initial release. Async outbound SMTP client primitives: MX resolution, DANE/STARTTLS, and response parsing — transport-agnostic for RFC 5321 senders.
