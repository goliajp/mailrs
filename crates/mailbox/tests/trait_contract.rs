//! Protocol-level integration tests for the [`MailboxStore`] trait.
//!
//! Drives every trait method against the in-memory reference impl. Acts as
//! both contract documentation (what each method must do) and the smell test
//! ("if a sane in-memory store needs gymnastics to satisfy a method, the
//! method is leaking a backend assumption").
//!
//! `tests/smoke.rs` covers the PG-specific path (testcontainers); this file
//! is the portable trait coverage.

use mailrs_mailbox::fixtures::{EXAMPLE_USER, InMemoryMailboxStore};
use mailrs_mailbox::{
    FLAG_ANSWERED, FLAG_DELETED, FLAG_FLAGGED, FLAG_SEEN, FlagOp, InsertMessage, MailboxStore,
    QueryFilter,
};

fn store() -> InMemoryMailboxStore {
    InMemoryMailboxStore::new()
}

fn sample_input<'a>(user: &'a str, mailbox: &'a str, uid_hint: u32) -> InsertMessage<'a> {
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
    let mut input = sample_input(EXAMPLE_USER, "INBOX", 1);
    input.flags = FLAG_SEEN | FLAG_FLAGGED;
    let inserted = s.insert_message(input).await.unwrap();
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
