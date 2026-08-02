//! Threading: the assigned thread and its ordering.

//! Protocol-level integration tests for the [`MailboxStore`] trait.
//!
//! Drives every trait method against the in-memory reference impl. Acts as
//! both contract documentation (what each method must do) and the smell test
//! ("if a sane in-memory store needs gymnastics to satisfy a method, the
//! method is leaking a backend assumption").
//!
//! `tests/smoke.rs` covers the PG-specific path (testcontainers); this file
//! is the portable trait coverage.

use mailrs_mailbox::MailboxStore;
use mailrs_mailbox::fixtures::EXAMPLE_USER;

use super::{sample_input, store};

#[tokio::test]
async fn thread_id_for_message_returns_assigned_thread() {
    let s = store();
    s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let mut input = sample_input(EXAMPLE_USER, "INBOX", 1);
    input.message_id = "abc@example.com";
    input.thread_id = "t-abc";
    s.insert_message(input).await.unwrap();
    let t = s
        .thread_id_for_message(EXAMPLE_USER, "abc@example.com")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(t, "t-abc");
}

#[tokio::test]
async fn thread_message_ids_orders_chronologically() {
    let s = store();
    s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let mut a = sample_input(EXAMPLE_USER, "INBOX", 1);
    a.thread_id = "t-1";
    a.internal_date = 100;
    let mut b = sample_input(EXAMPLE_USER, "INBOX", 2);
    b.thread_id = "t-1";
    b.internal_date = 50;
    let mut c = sample_input(EXAMPLE_USER, "INBOX", 3);
    c.thread_id = "t-1";
    c.internal_date = 200;
    let ia = s.insert_message(a).await.unwrap();
    let ib = s.insert_message(b).await.unwrap();
    let ic = s.insert_message(c).await.unwrap();

    let ids = s.thread_message_ids(EXAMPLE_USER, "t-1").await.unwrap();
    assert_eq!(ids, vec![ib.id, ia.id, ic.id]);
}

#[tokio::test]
async fn thread_references_returns_older_messages_newest_first() {
    let s = store();
    s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let mut a = sample_input(EXAMPLE_USER, "INBOX", 1);
    a.thread_id = "t-1";
    a.internal_date = 100;
    let mut b = sample_input(EXAMPLE_USER, "INBOX", 2);
    b.thread_id = "t-1";
    b.internal_date = 200;
    let mut c = sample_input(EXAMPLE_USER, "INBOX", 3);
    c.thread_id = "t-1";
    c.internal_date = 300;
    let ia = s.insert_message(a).await.unwrap();
    let ib = s.insert_message(b).await.unwrap();
    let ic = s.insert_message(c).await.unwrap();

    let refs = s.thread_references(ic.id).await.unwrap();
    assert_eq!(refs, vec![ib.id, ia.id], "newer-of-older first");
}

#[tokio::test]
async fn thread_references_returns_empty_for_singleton_thread() {
    let s = store();
    s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let inserted = s
        .insert_message(sample_input(EXAMPLE_USER, "INBOX", 1))
        .await
        .unwrap();
    let refs = s.thread_references(inserted.id).await.unwrap();
    assert!(refs.is_empty());
}

// ===== Changes (CONDSTORE / JMAP) =====
