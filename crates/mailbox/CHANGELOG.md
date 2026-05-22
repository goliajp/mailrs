# Changelog

All notable changes to `mailrs-mailbox` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.3] - 2026-05-22

### Added
- 25 inline corner-case tests for `InMemoryMailboxStore` covering flag ops, threading, and CONDSTORE bookkeeping edge cases.
- 62 docstrings closing the `#![deny(missing_docs)]` gate gap: 27 type fields, 15 analysis-op methods, 7 contact-op, 5 mailbox-op, 3 flag-op, 3 module-level, 2 message-op, 1 usage-op.
- 4 new perf gates: `bitmask_to_maildir_flags`, `InsertMessage::clone`, `QueryFilter` predicate, `resolve_thread_id`.
- README `## Performance` section with measured criterion medians: `add_flags` hot path ~55 ns, `extract_message_id` ~150 ns, `query_messages` 1k ~120 µs (fixture). M-series Mac, release profile, 100-sample.

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

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-mailbox-v1.0.3...HEAD
[1.0.3]: https://github.com/goliajp/mailrs/compare/mailrs-mailbox-v1.0.2...mailrs-mailbox-v1.0.3
[1.0.2]: https://github.com/goliajp/mailrs/compare/mailrs-mailbox-v1.0.1...mailrs-mailbox-v1.0.2
[1.0.1]: https://github.com/goliajp/mailrs/compare/mailrs-mailbox-v1.0.0...mailrs-mailbox-v1.0.1
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-mailbox-v1.0.0
