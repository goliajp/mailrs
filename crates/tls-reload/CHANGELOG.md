# Changelog

All notable changes to `mailrs-tls-reload` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0] - 2026-05-23

### Added

- Initial release. Extracted verbatim from `mailrs-server`'s `tls.rs`
  module (~50 LOC, ran in production for ~1 year) when re-auditing
  for misclassified cement. The `arc-swap` hot-reload pattern is
  universally useful for any rustls-terminating server; there's no
  reason it should live inside a mail server.
- `TlsState` — wrapper around `Arc<ArcSwap<ServerConfig>>` with
  `new` / `acceptor` / `swap` / `current` methods.
- `load_tls_config(cert_path, key_path)` — PEM file loader for
  the common no-client-auth case (RSA + EC keys, PKCS#1 / PKCS#8 /
  SEC1).
- 5 inline tests covering the loader's error paths (missing file,
  invalid PEM, valid cert + bad key, empty file, tempfile helper).

### Out of scope (deferred)

- Client-cert auth in the PEM loader. Construct your own
  `ServerConfig` for mTLS.
- File-watcher integration. Wrap with `notify` yourself.
- Cert-renewal driver. Use `instant-acme` / `acme-client` / `certbot`
  external to this crate.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-tls-reload-v1.0.0...HEAD
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-tls-reload-v1.0.0
