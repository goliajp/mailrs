# Changelog

All notable changes to `mailrs-mailbox` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `pg` feature (on by default) gating `PgMailboxStore` and the `sqlx`
  dependency. With `default-features = false` the crate is the
  `MailboxStore` trait, its types and the threading helpers, with no SQL
  driver. `spg` now implies `pg`.
- `ConversationSummary::snoozed_until` (epoch seconds, `0` when not
  snoozed). **Breaking** for code that builds `ConversationSummary` with
  a struct literal.
- `PgMailboxStore::wake_snoozed()`: un-archives snoozed threads whose
  time has come; returns the number woken.

### Fixed

- `snooze_thread` now archives the thread and records the wake time in
  one transaction, and `unsnooze_thread` clears both. Before, a snoozed
  conversation appeared in no view at all until it woke.
- Delivery writes the message's category, so the conversation list no
  longer reports every thread as `general`.
- A thread's unread state follows the delivered summary rather than the
  IMAP flags of a migrated payload, which had shown already-read mail
  from other senders as unread.
- The conversation list has a deterministic tie-break (thread id) for
  rows with the same timestamp, so paging no longer skips or repeats
  rows.

## [1.1.0] - 2026-08-04

### Added

- `spg` feature: swaps the storage backend from PostgreSQL to
  spg-embedded (an in-process SQL engine with the PostgreSQL dialect)
  through the `BackendPool` type alias.
- `PgMailboxStore::index_messages_batch`: index several messages in one
  transaction; returns the UIDs in input order, with the same UID and
  modseq bookkeeping as indexing them one by one. Empty input is a
  no-op.
- `PgMailboxStore::index_delivered`: index a message whose file has
  already been written by the caller; returns the UID.
- `PgMailboxStore::reconcile_maildir(maildir_root, dry_run)`: index every
  Maildir file that has no `messages` row and return a report.
- `PgMailboxStore::thread_root_subject` and `apply_subject_gate`: a reply
  joins its ancestor's thread only when the normalised subjects agree;
  otherwise it starts a new conversation.
- `PgMailboxStore::get_thread_maildir_ids` (newest first) and
  `set_thread_bucket` (sets the category of every message in a thread).

### Changed

- **Breaking:** `PgMailboxStore::count_unseen` returns
  `Result<i64, sqlx::Error>` instead of silently returning `0` on error.
- `index_message` is idempotent on `(mailbox_id, maildir_id)`.
- Indexing no longer takes `FOR UPDATE` on the mailbox row; UID and
  modseq allocation is a single atomic `UPDATE ... RETURNING`.
- The fallback search no longer uses a `tsvector` branch.
- The contacts table is `email_contacts` (was `contacts`).

### Fixed

- `insert_message` with flags passed the message id where the mailbox id
  was expected.
- Threads that contain only sent mail no longer appear in the inbox.
- Replies sent by the mailbox owner no longer change the date or
  position of the conversation in the inbox.
- The default conversation list no longer excludes threads categorised
  `spam` or `scam`; category is an opt-in filter only.
- A reconciled message with no extractable Message-ID gets a stable
  thread id derived from its Maildir id instead of an empty one, which
  had hidden it from every view.

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
