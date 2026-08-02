//! Protocol-level integration tests for the [`MailboxStore`] trait.
//!
//! Drives every trait method against the in-memory reference impl. Acts as
//! both contract documentation (what each method must do) and the smell test
//! ("if a sane in-memory store needs gymnastics to satisfy a method, the
//! method is leaking a backend assumption").
//!
//! `tests/smoke.rs` covers the PG-specific path (testcontainers); this file
//! is the portable trait coverage.

use mailrs_mailbox::InsertMessage;
use mailrs_mailbox::fixtures::InMemoryMailboxStore;

mod flags;
mod mailboxes;
mod messages;
mod queries;
mod threads;

pub(crate) fn store() -> InMemoryMailboxStore {
    InMemoryMailboxStore::new()
}

pub(crate) fn sample_input<'a>(
    user: &'a str,
    mailbox: &'a str,
    uid_hint: u32,
) -> InsertMessage<'a> {
    InsertMessage {
        user,
        mailbox_name: mailbox,
        blob_ref: "blob-x",
        sender: "Alice <alice@example.com>",
        recipients: "bob@example.com",
        subject: "hello",
        size: 256,
        date: 1_700_000_000,
        internal_date: 1_700_000_000 + uid_hint as i64,
        message_id: "msg-x@example.com",
        in_reply_to: "",
        thread_id: "t-x",
        flags: 0,
    }
}

// ===== Mailbox CRUD =====
