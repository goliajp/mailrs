//! Conversation-list queries (read-only).
//!
//! The two big SELECTs that build conversation summaries from grouped messages.
//! They share a projection shape and both answer some form of "give me the
//! inbox", and they were one file until it reached 510 of the 500 allowed
//! (`rules/common/file-size.md`). Split by query rather than by line count: each
//! file now holds one question and the whole of its answer.
//!
//! Flat, so no call site moved — the methods stay on `PgMailboxStore`, which is
//! what `impl` blocks in sibling modules give you for free.

mod by_ids;
mod list;
