# Changelog

All notable changes to `mailrs-tls-reload` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.0] - 2026-10-22

### Added

- `CertWatcher`: follows a certificate and key on disk and swaps new
  contents into a `TlsState`. It compares file contents, not
  timestamps, so an atomic `mv`, a copy that keeps the old mtime and an
  in-place rewrite are all picked up. `check` returns `Ok(true)` when a
  new certificate is being served; on error (including a new
  certificate paired with the old key) the old pair stays in service.
- `spawn_watch(state, watcher, every, service)`: runs `check` on a
  tokio interval and logs each swap and failure with `tracing`.
- `config_from_pem(cert, key)`: build a `ServerConfig` from PEM bytes
  already in memory.
- New dependencies: `tokio` (`rt`, `time`) and `tracing`.

## [1.0.2] - 2026-08-04

### Changed

- Source formatting only; no API or behaviour change.

## [1.0.1] - 2026-05-22

### Changed

- Tests for `TlsState` `swap` / `current` / `acceptor` with real
  self-signed certificates, and parallel-safe temporary file names in
  the tests. No API or behaviour change.

## [1.0.0] - 2026-05-22

### Added

- Initial release. Extracted verbatim from the mailrs server's TLS
  module (~50 LOC, in production for about a year). The `arc-swap`
  hot-reload pattern is
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
