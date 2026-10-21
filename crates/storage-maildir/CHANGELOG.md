# Changelog

All notable changes to `mailrs-maildir` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `Maildir::flags_of(id)`: the standard flags currently on a message's
  file (`Some(vec![])` for a message still in `new/`, `None` when there
  is no file).
- Keyword bits (the lowercase letters in the `:2,` suffix):
  `Maildir::keywords_of(id)`, `Maildir::mark_processed_with_keywords`,
  and the free functions `keywords_of(info)` and
  `serialize_flags_and_keywords(flags, keywords)`.

### Changed

- `Maildir::mark_processed` keeps the keyword bits already on the file
  instead of dropping them when it rewrites the flag suffix.

## [1.3.0] - 2026-08-04

### Added

- `Maildir::fetch(id)`: read a message's bytes from `new/` or `cur/`;
  `Ok(None)` when no file exists.
- `Maildir::mark_processed(id, flags)`: move a message from `new/` to
  `cur/` with the given flags, or update its flags if it is already in
  `cur/`.
- `Maildir::delete(id)`: remove a message from `new/` or `cur/`.
- `locate(user_root, blob_ref)`: resolve a stored message reference
  (including Maildir++ subfolders such as `.Sent/<id>`) to a `Maildir`
  and `MessageId`.
- `Maildir::path()`.

### Fixed

- Lookups by id match files whose names carry a `:2,FLAGS` suffix, so an
  id stays valid after its flags change.

## [1.2.1] - 2026-05-29

### Changed

- `deliver` persists the file with `sync_data` (fdatasync on Linux)
  instead of `sync_all`, saving one journal write per delivery. On macOS
  the two are identical.

## [1.2.0] - 2026-05-24

### Added

- `Maildir::deliver_batch(&[&[u8]])`: deliver several messages with a
  single directory fsync; returns the ids in input order. Empty input
  makes no system calls.

## [1.1.0] - 2026-05-24

### Added

- `Maildir::create_cached(path)`: like `create`, but remembers per path
  that `tmp/`, `new/` and `cur/` exist and skips `create_dir_all` on
  later calls. `Maildir::invalidate_cache(path)` forgets a path.
- New dependency: `dashmap` 6.

## [1.0.2] - 2026-05-22

### Added
- `#![deny(missing_docs)]` gate; all public items now carry rustdoc.
- Perf regression gates under `tests/perf_gate.rs` with documented budgets in `BUDGETS.md`.

### Changed
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.
- Refreshed dev dependencies (criterion 0.7 → 0.8).

## [1.0.1] - 2026-05-20

### Added
- Criterion benchmarks for atomic delivery, directory scans, and flag parsing.

## [1.0.0] - 2026-05-19

### Added
- Initial release. Maildir filesystem-format primitives: atomic delivery, directory scans, and flag parsing. No IMAP or POP3 layer.
