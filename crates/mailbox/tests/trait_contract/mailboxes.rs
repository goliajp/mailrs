//! Mailbox lifecycle: create, delete, rename, list, status.

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
use mailrs_mailbox::{FLAG_SEEN, MailboxStore};

use super::{sample_input, store};

#[tokio::test]
async fn create_mailbox_is_idempotent_on_same_name() {
    let s = store();
    let first = s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let second = s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    assert_eq!(first.id, second.id, "second create returns existing");
    assert_eq!(first.name, "INBOX");
}

#[tokio::test]
async fn create_mailbox_assigns_unique_ids_per_user_name_pair() {
    let s = store();
    let inbox = s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let sent = s.create_mailbox(EXAMPLE_USER, "Sent").await.unwrap();
    assert_ne!(inbox.id, sent.id);
}

#[tokio::test]
async fn delete_mailbox_returns_true_when_removed_false_when_missing() {
    let s = store();
    s.create_mailbox(EXAMPLE_USER, "Junk").await.unwrap();
    assert!(s.delete_mailbox(EXAMPLE_USER, "Junk").await.unwrap());
    assert!(!s.delete_mailbox(EXAMPLE_USER, "Junk").await.unwrap());
}

#[tokio::test]
async fn delete_mailbox_cascades_to_its_messages() {
    let s = store();
    s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let _ = s
        .insert_message(sample_input(EXAMPLE_USER, "INBOX", 1))
        .await
        .unwrap();
    s.delete_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let inbox = s.get_mailbox(EXAMPLE_USER, "INBOX").await.unwrap().unwrap();
    let status = s.mailbox_status(inbox.id).await.unwrap();
    assert_eq!(status.total, 0, "messages cascaded with deleted mailbox");
}

#[tokio::test]
async fn rename_mailbox_renames_existing() {
    let s = store();
    s.create_mailbox(EXAMPLE_USER, "Archive").await.unwrap();
    s.rename_mailbox(EXAMPLE_USER, "Archive", "Old")
        .await
        .unwrap();
    assert!(
        s.get_mailbox(EXAMPLE_USER, "Archive")
            .await
            .unwrap()
            .is_none()
    );
    assert!(s.get_mailbox(EXAMPLE_USER, "Old").await.unwrap().is_some());
}

#[tokio::test]
async fn rename_mailbox_errors_when_missing() {
    let s = store();
    assert!(
        s.rename_mailbox(EXAMPLE_USER, "Nope", "Whatever")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn list_mailboxes_returns_only_user_owned() {
    let s = store();
    s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    s.create_mailbox(EXAMPLE_USER, "Sent").await.unwrap();
    s.create_mailbox("bob@example.com", "INBOX").await.unwrap();
    let mine = s.list_mailboxes(EXAMPLE_USER).await.unwrap();
    assert_eq!(mine.len(), 2);
    let bobs = s.list_mailboxes("bob@example.com").await.unwrap();
    assert_eq!(bobs.len(), 1);
}

#[tokio::test]
async fn get_mailbox_by_id_round_trips_create() {
    let s = store();
    let mb = s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let by_id = s.get_mailbox_by_id(mb.id).await.unwrap().unwrap();
    assert_eq!(by_id.name, "INBOX");
}

#[tokio::test]
async fn mailbox_status_counts_total_and_unread() {
    let s = store();
    let mb = s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let _ = s
        .insert_message(sample_input(EXAMPLE_USER, "INBOX", 1))
        .await
        .unwrap();
    let _ = s
        .insert_message(sample_input(EXAMPLE_USER, "INBOX", 2))
        .await
        .unwrap();
    // mark one as seen
    s.add_flags(mb.id, 1, FLAG_SEEN).await.unwrap();
    let status = s.mailbox_status(mb.id).await.unwrap();
    assert_eq!(status.total, 2);
    assert_eq!(status.unread, 1);
    assert_eq!(status.recent, 0, "in-memory impl doesn't track recency");
}

// ===== Message insert + lookup =====
