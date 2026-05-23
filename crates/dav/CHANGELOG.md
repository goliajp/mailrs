# Changelog

All notable changes to `mailrs-dav` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.1] - 2026-05-23

### Added

- 39 new inline tests across `carddav.rs` (+22) and `caldav.rs` (+17)
  covering precondition handling and report semantics:
  - Contact / event PUT: `201` on create vs `204` on update,
    `If-Match` correct/wrong etag, `If-None-Match: *` for new vs
    existing resources.
  - Address book / calendar PROPFIND: `Depth: 0` returns only the
    collection, `Depth: 1` lists children, default-collection
    creation is idempotent across repeated home-PROPFINDs.
  - REPORT multiget with empty UID list, with missing UIDs, and
    `calendar-query` / `addressbook-query` no-filter forms.
  - DELETE on missing event/contact returns `NotFound`.
  - `urlencode` byte-level assertions for alphanumeric pass-through,
    space encoding, special chars, and per-byte UTF-8 (Japanese).
- Lib test count: 44 → 83.

No lib code change.

## [1.1.0] - 2026-05-21

### Added
- Public in-memory `CalendarStore` / `AddressBookStore` fixtures, promoted from `dev-dependencies` so downstream crates can reuse them in their own tests and examples.
- Perf regression gates under `tests/perf_gate.rs` covering handler dispatch and composition, with documented budgets in `BUDGETS.md`.
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.

## [1.0.3] - 2026-05-21

### Added
- Criterion benchmarks for handler dispatch and request composition under `benches/`.

## [1.0.2] - 2026-05-21

### Added
- Protocol-level integration coverage: end-to-end CalDAV and CardDAV scenarios against the in-memory store.

## [1.0.1] - 2026-05-21

### Added
- `#![deny(missing_docs)]` gate; all 22 public items now carry rustdoc.
- Expanded README with quick-start example and trait overview.

## [1.0.0] - 2026-05-20

### Added
- Initial release. Framework-agnostic CalDAV (RFC 4791) and CardDAV (RFC 6352) server-side handlers, with the data layer pluggable via the `CalendarStore` / `AddressBookStore` traits.

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-dav-v1.1.0...HEAD
[1.1.0]: https://github.com/goliajp/mailrs/compare/mailrs-dav-v1.0.3...mailrs-dav-v1.1.0
[1.0.3]: https://github.com/goliajp/mailrs/compare/mailrs-dav-v1.0.2...mailrs-dav-v1.0.3
[1.0.2]: https://github.com/goliajp/mailrs/compare/mailrs-dav-v1.0.1...mailrs-dav-v1.0.2
[1.0.1]: https://github.com/goliajp/mailrs/compare/mailrs-dav-v1.0.0...mailrs-dav-v1.0.1
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-dav-v1.0.0
