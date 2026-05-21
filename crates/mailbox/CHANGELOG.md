# Changelog

All notable changes to `mailrs-mailbox` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.2] - 2026-05-22

### Added
- `#![deny(missing_docs)]` and `#![deny(rustdoc::broken_intra_doc_links)]` gates (closing an audit gap — the other 14 crates already had them).
- 62 new docstrings exposed by the gate: 27 type fields, 15 analysis-op methods, 7 contact-op methods, 5 mailbox-op methods, 3 flag-op methods, 3 module-level docs, 2 message-op methods, 1 usage-op method. All anchored to existing sibling docs and README semantics.

## [1.0.1] - 2026-05-22

### Added
- 25 inline corner-case tests for `InMemoryMailboxStore` covering edge cases in flag ops, threading, and CONDSTORE bookkeeping.

## [1.0.0] - 2026-05-21

### Added
- Initial release. Mailbox metadata storage abstraction for IMAP/JMAP servers: `MailboxStore` trait + PostgreSQL reference implementation + `InMemoryMailboxStore` test fixture. CONDSTORE, threading, flag ops, and change tracking. Introduced via a multi-stage refactor (trait extraction, PG impl, in-memory impl + trait contract tests, README + crate-level docs).

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-mailbox-v1.0.2...HEAD
[1.0.2]: https://github.com/goliajp/mailrs/compare/mailrs-mailbox-v1.0.1...mailrs-mailbox-v1.0.2
[1.0.1]: https://github.com/goliajp/mailrs/compare/mailrs-mailbox-v1.0.0...mailrs-mailbox-v1.0.1
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-mailbox-v1.0.0
