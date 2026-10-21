# Changelog

All notable changes to `mailrs-sieve` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.0.1] - 2026-08-04

### Changed

- Depends on `mailrs-sieve-core` 0.3 (faster matching, same results).

## [2.0.0] - 2026-06-02

### Changed (BREAKING)

- The engine is now `mailrs-sieve-core` (Apache-2.0 OR MIT) instead of
  `sieve-rs`, so the crate no longer pulls in AGPL code.
- `SieveAction::Keep` and `SieveAction::FileInto` carry RFC 5232
  imap4flags: `Keep { flags }`, `FileInto { mailbox, flags }`.
- `SieveAction::Vacation` carries the parsed parameters (`reason`,
  `subject`, `from`, `handle`, `period_secs`, `addresses`, `mime`)
  instead of a ready-made reply message. Building the reply and
  de-duplicating it (RFC 5230 §4.6) is the caller's job.
- `CompiledSieve` is this crate's own type. `compile_sieve` validates the
  script (RFC 5228 §3.2): a script that uses an extension without naming
  it in `require` is rejected.

### Added

- `imap4flags` (`setflag`, `addflag`, `removeflag`, `hasflag`, `:flags`)
  and `subaddress` (`:user`, `:detail`).

## [1.0.0] - 2026-05-24

### Added

- Initial release: a delivery-action wrapper over the `sieve-rs` engine.
- `compile_sieve(script)`, `evaluate_sieve(&compiled, message)` and
  `evaluate_sieve_with_envelope(&compiled, message, from, to)`.
- `SieveAction`: `Keep`, `FileInto(String)`, `Discard`,
  `Redirect(String)`, `Reject(String)`, `Vacation(recipient, message)`.
