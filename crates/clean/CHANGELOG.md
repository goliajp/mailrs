# Changelog

All notable changes to `mailrs-clean` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.2.0] - 2026-10-22

### Added

- `preview_line(text, max)`: the one-line preview shown under a subject
  in a message list. Whitespace runs (including Unicode spaces) collapse
  to one space, zero-width padding is removed while the zero-width
  joiner is kept, rule lines drawn with dashes or box-drawing characters
  are dropped, `&nbsp;` / `&zwnj;` written into plain text are treated
  as the characters they name, and a cut is marked with an ellipsis.
- `parse_unsubscribe(value, post)` and `Unsubscribe`: read
  `List-Unsubscribe` (RFC 2369) and `List-Unsubscribe-Post` (RFC 8058)
  for where unsubscribing goes and whether one-click applies.
- `html_to_clean_text` is public: HTML rendered as paragraph-aware plain
  text, with nothing removed.

## [1.1.0] - 2026-08-04

### Changed

- Faster HTML cleaning: comments and block tags are stripped in a single
  pass (about 44% faster on a 5 KB message), and tracking-pixel,
  bulk-sender and automated-sender detection use ASCII case folding and
  `memchr` scans. New dependency: `memchr`.
- Internal split into modules; the public API is unchanged.

## [1.0.3] - 2026-05-22

### Added
- README `## Performance` section with measured criterion medians: `clean_email_html(5KB)` ~336 µs, `clean_email_html(50KB)` ~2.5 ms (~22 MB/s). M-series Mac, release profile, 100-sample.

## [1.0.2] - 2026-05-22

### Added
- Perf regression gates under `tests/perf_gate.rs` with documented budgets in `BUDGETS.md`.
- `#![deny(missing_docs)]` gate; all public items now carry rustdoc.

### Changed
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.
- Refreshed dev dependencies (criterion 0.7 → 0.8).

## [1.0.1] - 2026-05-20

### Fixed
- Corrected the `repository` URL typo in `Cargo.toml` so crates.io links resolve.

## [1.0.0] - 2026-05-20

### Added
- Initial release. Zero-I/O email content cleanup primitives: HTML sanitization, tracking-pixel detection, bulk/automated-sender heuristics, and quoted-reply splitting.
