# Changelog

All notable changes to `mailrs-smtp-proto` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.2.0] - 2026-10-22

### Added

- `Session::pending_recipient(&Command) -> Option<String>`: the forward
  path a RCPT command would add in the current state, or `None` when it
  would add none. A caller can refuse that recipient with its own reply
  and not pass the command on, leaving the transaction unchanged as
  RFC 5321 §3.3 requires for a rejected RCPT.

## [1.1.0] - 2026-08-04

### Changed

- DATA dot-unstuffing scans for line ends with `memchr`, which is the
  per-line cost on every inbound message body. Output is unchanged.
- New dependency: `memchr` 2.

## [1.0.5] - 2026-05-29

### Changed

- Command parsing no longer allocates to match the verb: verbs and AUTH
  mechanisms are uppercased into a stack buffer, and the `FROM:` / `TO:`
  prefixes of MAIL and RCPT are compared case-insensitively in place.
  `DATA` parses about 4× faster, `MAIL FROM` about 27% and `RCPT TO`
  about 20%. No API change.

## [1.0.4] - 2026-05-22

### Changed

- `format_ehlo_response` builds the reply into one pre-sized `String`
  without `write!`, about 9× faster (307 ns to 35 ns). Output is
  unchanged.

## [1.0.3] - 2026-05-22

### Added
- README `## Performance` section with measured criterion medians: `parse_command(EHLO)` ~22 ns, `parse_command(MAIL FROM)` ~103 ns, `format_ehlo_response` ~307 ns. M-series Mac, release profile, 100-sample.

## [1.0.2] - 2026-05-22

### Added
- `#![deny(missing_docs)]` gate; all public items now carry rustdoc.
- Perf regression gates under `tests/perf_gate.rs` with documented budgets in `BUDGETS.md`.

### Changed
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.
- Refreshed dev dependencies (criterion 0.7 → 0.8).

## [1.0.1] - 2026-05-20

### Added
- Criterion benchmarks for command parser and response formatter hot paths.

## [1.0.0] - 2026-05-19

### Added
- Initial release. Zero-I/O, async-runtime-agnostic SMTP protocol parser, formatter, and session state machine (RFC 5321).
