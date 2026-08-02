//! Message insert, read, copy, move and expunge.

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
use mailrs_mailbox::{FLAG_DELETED, FLAG_FLAGGED, FLAG_SEEN, MailboxStore};

use super::{sample_input, store};

#[tokio::test]
async fn insert_message_allocates_monotonic_uids_and_bumps_modseq() {
    let s = store();
    s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let first = s
        .insert_message(sample_input(EXAMPLE_USER, "INBOX", 1))
        .await
        .unwrap();
    let second = s
        .insert_message(sample_input(EXAMPLE_USER, "INBOX", 2))
        .await
        .unwrap();
    assert_eq!(first.uid, 1);
    assert_eq!(second.uid, 2, "uid is monotonic");
    assert!(
        second.modseq > first.modseq,
        "modseq is strictly increasing"
    );
}

#[tokio::test]
async fn insert_message_with_initial_flags_persists_them() {
    let s = store();
    s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    // Insert plain messages first so the flagged message's row id (3) is
    // NOT equal to the mailbox id (1). A single-message store hid a real
    // bug for a long time: insert_message's flags path passed the MESSAGE
    // id where set_flags expects the MAILBOX id, and with one message the
    // two ids coincidentally matched. With id divergence, that bug makes
    // this insert fail ("no rows returned" from bump_modseq).
    s.insert_message(sample_input(EXAMPLE_USER, "INBOX", 1))
        .await
        .unwrap();
    s.insert_message(sample_input(EXAMPLE_USER, "INBOX", 2))
        .await
        .unwrap();
    let mut input = sample_input(EXAMPLE_USER, "INBOX", 3);
    input.flags = FLAG_SEEN | FLAG_FLAGGED;
    let inserted = s.insert_message(input).await.unwrap();
    assert!(
        inserted.id > 1,
        "flagged message id must diverge from mailbox id"
    );
    let msg = s.get_message(inserted.id).await.unwrap().unwrap();
    assert_eq!(msg.flags, FLAG_SEEN | FLAG_FLAGGED);
}

#[tokio::test]
async fn insert_message_into_unknown_mailbox_errors() {
    let s = store();
    let err = s
        .insert_message(sample_input(EXAMPLE_USER, "Missing", 1))
        .await;
    assert!(err.is_err());
}

#[tokio::test]
async fn get_message_by_uid_returns_some_then_none_after_expunge() {
    let s = store();
    let mb = s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let inserted = s
        .insert_message(sample_input(EXAMPLE_USER, "INBOX", 1))
        .await
        .unwrap();
    assert!(
        s.get_message_by_uid(mb.id, inserted.uid)
            .await
            .unwrap()
            .is_some()
    );
    s.add_flags(mb.id, inserted.uid, FLAG_DELETED)
        .await
        .unwrap();
    s.expunge(mb.id).await.unwrap();
    assert!(
        s.get_message_by_uid(mb.id, inserted.uid)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn get_message_by_id_returns_message_with_user_address_filled() {
    let s = store();
    s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let inserted = s
        .insert_message(sample_input(EXAMPLE_USER, "INBOX", 1))
        .await
        .unwrap();
    let msg = s.get_message(inserted.id).await.unwrap().unwrap();
    assert_eq!(msg.user_address, EXAMPLE_USER);
}

#[tokio::test]
async fn find_by_message_id_searches_within_user_only() {
    let s = store();
    s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    s.create_mailbox("bob@example.com", "INBOX").await.unwrap();
    let mut alice_input = sample_input(EXAMPLE_USER, "INBOX", 1);
    alice_input.message_id = "shared-id@example.com";
    s.insert_message(alice_input).await.unwrap();
    let mut bob_input = sample_input("bob@example.com", "INBOX", 1);
    bob_input.message_id = "shared-id@example.com";
    s.insert_message(bob_input).await.unwrap();

    let alice_msg = s
        .find_by_message_id(EXAMPLE_USER, "shared-id@example.com")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(alice_msg.user_address, EXAMPLE_USER);
    let bob_msg = s
        .find_by_message_id("bob@example.com", "shared-id@example.com")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bob_msg.user_address, "bob@example.com");
}

// ===== copy / move =====

#[tokio::test]
async fn copy_message_keeps_source_and_adds_new_uid_in_destination() {
    let s = store();
    let src = s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let dst = s.create_mailbox(EXAMPLE_USER, "Archive").await.unwrap();
    let inserted = s
        .insert_message(sample_input(EXAMPLE_USER, "INBOX", 1))
        .await
        .unwrap();

    let new_uid = s.copy_message(src.id, inserted.uid, dst.id).await.unwrap();
    assert_eq!(new_uid, 1, "destination uidnext starts at 1");
    assert_eq!(s.mailbox_status(src.id).await.unwrap().total, 1);
    assert_eq!(s.mailbox_status(dst.id).await.unwrap().total, 1);
}

#[tokio::test]
async fn move_message_removes_source_and_adds_destination() {
    let s = store();
    let src = s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let dst = s.create_mailbox(EXAMPLE_USER, "Archive").await.unwrap();
    let inserted = s
        .insert_message(sample_input(EXAMPLE_USER, "INBOX", 1))
        .await
        .unwrap();

    s.move_message(src.id, inserted.uid, dst.id).await.unwrap();
    assert_eq!(s.mailbox_status(src.id).await.unwrap().total, 0);
    assert_eq!(s.mailbox_status(dst.id).await.unwrap().total, 1);
}

#[tokio::test]
async fn copy_missing_source_errors() {
    let s = store();
    let src = s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let dst = s.create_mailbox(EXAMPLE_USER, "Archive").await.unwrap();
    assert!(s.copy_message(src.id, 999, dst.id).await.is_err());
}

#[tokio::test]
async fn expunge_returns_deleted_uids_in_ascending_order() {
    let s = store();
    let mb = s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    for i in 1..=3 {
        s.insert_message(sample_input(EXAMPLE_USER, "INBOX", i as u32))
            .await
            .unwrap();
    }
    s.add_flags(mb.id, 3, FLAG_DELETED).await.unwrap();
    s.add_flags(mb.id, 1, FLAG_DELETED).await.unwrap();
    let removed = s.expunge(mb.id).await.unwrap();
    assert_eq!(removed, vec![1, 3]);
}

// ===== Flags =====
