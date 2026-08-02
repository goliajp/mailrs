//! Flag mutation, including the CONDSTORE unchanged-since path.

//! Protocol-level integration tests for the [`MailboxStore`] trait.
//!
//! Drives every trait method against the in-memory reference impl. Acts as
//! both contract documentation (what each method must do) and the smell test
//! ("if a sane in-memory store needs gymnastics to satisfy a method, the
//! method is leaking a backend assumption").
//!
//! `tests/smoke.rs` covers the PG-specific path (testcontainers); this file
//! is the portable trait coverage.

use mailrs_mailbox::fixtures::EXAMPLE_USER;
use mailrs_mailbox::{FLAG_ANSWERED, FLAG_FLAGGED, FLAG_SEEN, FlagOp, MailboxStore};

use super::{sample_input, store};

#[tokio::test]
async fn set_flags_replaces_bitmask_and_bumps_modseq() {
    let s = store();
    let mb = s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let inserted = s
        .insert_message(sample_input(EXAMPLE_USER, "INBOX", 1))
        .await
        .unwrap();
    let modseq = s
        .set_flags(mb.id, 1, FLAG_SEEN | FLAG_ANSWERED)
        .await
        .unwrap();
    assert!(modseq > inserted.modseq);
    let msg = s.get_message_by_uid(mb.id, 1).await.unwrap().unwrap();
    assert_eq!(msg.flags, FLAG_SEEN | FLAG_ANSWERED);
}

#[tokio::test]
async fn add_flags_ors_and_remove_flags_and_nots() {
    let s = store();
    let mb = s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    s.insert_message(sample_input(EXAMPLE_USER, "INBOX", 1))
        .await
        .unwrap();
    s.add_flags(mb.id, 1, FLAG_SEEN).await.unwrap();
    s.add_flags(mb.id, 1, FLAG_FLAGGED).await.unwrap();
    let msg = s.get_message_by_uid(mb.id, 1).await.unwrap().unwrap();
    assert_eq!(msg.flags, FLAG_SEEN | FLAG_FLAGGED);

    s.remove_flags(mb.id, 1, FLAG_SEEN).await.unwrap();
    let msg = s.get_message_by_uid(mb.id, 1).await.unwrap().unwrap();
    assert_eq!(msg.flags, FLAG_FLAGGED);
}

#[tokio::test]
async fn store_flags_if_unchanged_succeeds_on_matching_modseq() {
    let s = store();
    let mb = s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let inserted = s
        .insert_message(sample_input(EXAMPLE_USER, "INBOX", 1))
        .await
        .unwrap();
    let result = s
        .store_flags_if_unchanged(mb.id, 1, FlagOp::Add, FLAG_SEEN, inserted.modseq)
        .await
        .unwrap();
    assert!(result.is_some(), "modseq <= unchangedsince → success");
}

#[tokio::test]
async fn store_flags_if_unchanged_returns_none_on_stale_unchangedsince() {
    let s = store();
    let mb = s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    s.insert_message(sample_input(EXAMPLE_USER, "INBOX", 1))
        .await
        .unwrap();
    s.add_flags(mb.id, 1, FLAG_SEEN).await.unwrap();
    let stale_modseq = 0u64; // any modseq below the current
    let result = s
        .store_flags_if_unchanged(mb.id, 1, FlagOp::Add, FLAG_FLAGGED, stale_modseq)
        .await
        .unwrap();
    assert!(
        result.is_none(),
        "precondition fails when modseq has advanced"
    );
    // verify the flag was NOT applied
    let msg = s.get_message_by_uid(mb.id, 1).await.unwrap().unwrap();
    assert_eq!(msg.flags & FLAG_FLAGGED, 0);
}

// ===== Threads =====
