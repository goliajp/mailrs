//! Search, modseq deltas, and the per-user storage total.

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
use mailrs_mailbox::{FLAG_FLAGGED, FLAG_SEEN, MailboxStore, QueryFilter};

use super::{sample_input, store};

#[tokio::test]
async fn messages_changed_since_returns_only_strictly_greater_modseq() {
    let s = store();
    let mb = s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let m1 = s
        .insert_message(sample_input(EXAMPLE_USER, "INBOX", 1))
        .await
        .unwrap();
    let m2 = s
        .insert_message(sample_input(EXAMPLE_USER, "INBOX", 2))
        .await
        .unwrap();

    let changes = s.messages_changed_since(mb.id, m1.modseq).await.unwrap();
    assert_eq!(changes.len(), 1, "only m2 is > m1.modseq");
    assert_eq!(changes[0].uid, m2.uid);
}

#[tokio::test]
async fn messages_changed_since_orders_by_modseq_ascending() {
    let s = store();
    let mb = s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    for i in 1..=3 {
        s.insert_message(sample_input(EXAMPLE_USER, "INBOX", i))
            .await
            .unwrap();
    }
    // mutate flags in reverse order so modseq doesn't follow uid
    s.add_flags(mb.id, 3, FLAG_SEEN).await.unwrap();
    s.add_flags(mb.id, 1, FLAG_FLAGGED).await.unwrap();
    let changes = s.messages_changed_since(mb.id, 0).await.unwrap();
    let modseqs: Vec<u64> = changes.iter().map(|m| m.modseq).collect();
    assert!(
        modseqs.windows(2).all(|w| w[0] <= w[1]),
        "result is modseq-ascending"
    );
}

// ===== Query =====

#[tokio::test]
async fn query_messages_filters_by_mailbox() {
    let s = store();
    let a = s.create_mailbox(EXAMPLE_USER, "A").await.unwrap();
    s.create_mailbox(EXAMPLE_USER, "B").await.unwrap();
    s.insert_message(sample_input(EXAMPLE_USER, "A", 1))
        .await
        .unwrap();
    s.insert_message(sample_input(EXAMPLE_USER, "B", 1))
        .await
        .unwrap();
    let f = QueryFilter {
        mailbox_id: Some(a.id),
        user: Some(EXAMPLE_USER),
        limit: 50,
        ..Default::default()
    };
    let out = s.query_messages(f).await.unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].mailbox_id, a.id);
}

#[tokio::test]
async fn query_messages_text_matches_case_insensitive_across_three_fields() {
    let s = store();
    s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    let mut a = sample_input(EXAMPLE_USER, "INBOX", 1);
    a.subject = "Quarterly Report";
    let mut b = sample_input(EXAMPLE_USER, "INBOX", 2);
    b.sender = "Bob <bob@example.com>";
    let mut c = sample_input(EXAMPLE_USER, "INBOX", 3);
    c.recipients = "team@example.com";
    s.insert_message(a).await.unwrap();
    s.insert_message(b).await.unwrap();
    s.insert_message(c).await.unwrap();

    let f = QueryFilter {
        user: Some(EXAMPLE_USER),
        text: Some("REPORT"),
        limit: 50,
        ..Default::default()
    };
    let r = s.query_messages(f).await.unwrap();
    assert_eq!(r.len(), 1);

    let f = QueryFilter {
        user: Some(EXAMPLE_USER),
        text: Some("team@example.com"),
        limit: 50,
        ..Default::default()
    };
    let r = s.query_messages(f).await.unwrap();
    assert_eq!(r.len(), 1);
}

#[tokio::test]
async fn query_messages_keyword_filters_compose() {
    let s = store();
    let mb = s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    for i in 1..=4 {
        s.insert_message(sample_input(EXAMPLE_USER, "INBOX", i))
            .await
            .unwrap();
    }
    s.add_flags(mb.id, 1, FLAG_SEEN).await.unwrap();
    s.add_flags(mb.id, 2, FLAG_SEEN).await.unwrap();
    s.add_flags(mb.id, 3, FLAG_FLAGGED).await.unwrap();

    // has $seen AND not $flagged → only msgs 1, 2
    let f = QueryFilter {
        user: Some(EXAMPLE_USER),
        has_keyword: Some(FLAG_SEEN),
        not_keyword: Some(FLAG_FLAGGED),
        limit: 50,
        ..Default::default()
    };
    let r = s.query_messages(f).await.unwrap();
    assert_eq!(r.len(), 2);
}

#[tokio::test]
async fn query_messages_paginates_with_position_and_limit() {
    let s = store();
    s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    for i in 1..=5 {
        s.insert_message(sample_input(EXAMPLE_USER, "INBOX", i))
            .await
            .unwrap();
    }
    let f = QueryFilter {
        user: Some(EXAMPLE_USER),
        position: 1,
        limit: 2,
        ..Default::default()
    };
    let r = s.query_messages(f).await.unwrap();
    assert_eq!(r.len(), 2);
}

// ===== Quota =====

#[tokio::test]
async fn user_storage_bytes_sums_message_sizes() {
    let s = store();
    s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    for i in 1..=3 {
        let mut input = sample_input(EXAMPLE_USER, "INBOX", i);
        input.size = 100;
        s.insert_message(input).await.unwrap();
    }
    let bytes = s.user_storage_bytes(EXAMPLE_USER).await.unwrap();
    assert_eq!(bytes, 300);
}

#[tokio::test]
async fn user_storage_bytes_isolated_per_user() {
    let s = store();
    s.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
    s.create_mailbox("bob@example.com", "INBOX").await.unwrap();
    let mut alice_input = sample_input(EXAMPLE_USER, "INBOX", 1);
    alice_input.size = 100;
    s.insert_message(alice_input).await.unwrap();
    let mut bob_input = sample_input("bob@example.com", "INBOX", 1);
    bob_input.size = 999;
    s.insert_message(bob_input).await.unwrap();
    assert_eq!(s.user_storage_bytes(EXAMPLE_USER).await.unwrap(), 100);
    assert_eq!(s.user_storage_bytes("bob@example.com").await.unwrap(), 999);
}
