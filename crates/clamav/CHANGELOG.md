# Changelog

All notable changes to `mailrs-clamav` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0] - 2026-05-23

### Added

- Initial release. Extracted from `mailrs-server`'s
  `inbound::content_scan::scan_clamav` (~80 LOC, ran in production for
  ~1 year) and extended with PING + VERSION + caller-supplied timeout.
- `scan(addr, data)` — zINSTREAM scan with `DEFAULT_TIMEOUT` (30s).
- `scan_with_timeout(addr, data, timeout)` — same with custom timeout.
- `ping(addr, timeout) -> bool` — zPING health check.
- `version(addr, timeout) -> Option<String>` — zVERSION daemon
  version query.
- `parse_response(bytes)` — exposed reply parser for testing and
  for re-use without going through the socket.
- `ClamavResult` enum (`Clean` / `Virus(name)` / `Error(desc)`).
- `CHUNK_SIZE` (2 MiB) + `DEFAULT_TIMEOUT` (30s) public constants.
- 16 inline unit tests covering: clean reply (with + without trailing
  NUL, with + without trailing newline), virus reply (short name,
  long name with dots/dashes, no `stream:` prefix), error reply
  (size limit, empty, whitespace-only), invalid-UTF-8 lossy parse,
  unreachable-address Error, zero-timeout Error, PING + VERSION
  unreachable-returns-None.
- `tests/perf_gate.rs` with 3 regression budgets.
- `benches/clamav.rs` with 6 criterion benchmark functions.

### Out of scope (deferred)

- Connection pool / re-use. Each `scan` opens a fresh TCP connection;
  acceptable on localhost ClamAV deployments.
- Unix domain socket support. TCP only in 1.0.
- `MULTISCAN` / `CONTSCAN` / `STATS` / `RELOAD`. Admin-only commands
  out of scope.
- `SCAN` with filesystem path. INSTREAM only — safer for sandboxed
  deployments.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-clamav-v1.0.0...HEAD
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-clamav-v1.0.0
