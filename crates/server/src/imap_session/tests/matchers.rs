//! Unit tests for `message_matches_criteria` — the AND-list
//! SEARCH key evaluator that drives both SEARCH and the
//! SORT pre-filter. Each test synthesizes a `MessageMeta`,
//! matches against one or more `SearchKey`s, and asserts
//! match-or-not.

use mailrs_imap_proto::SearchKey;
use mailrs_mailbox::{
    FLAG_ANSWERED, FLAG_DELETED, FLAG_DRAFT, FLAG_FLAGGED, FLAG_RECENT, FLAG_SEEN,
};

use crate::imap_session::search::message_matches_criteria;

// ===== unit tests for pure helper functions =====

fn make_msg(overrides: impl FnOnce(&mut mailrs_mailbox::MessageMeta)) -> mailrs_mailbox::MessageMeta {
    let mut msg = mailrs_mailbox::MessageMeta {
        id: 1,
        mailbox_id: 1,
        uid: 42,
        maildir_id: "test".into(),
        sender: "alice@example.com".into(),
        recipients: "bob@example.com".into(),
        subject: "Hello World".into(),
        date: 1700000000,
        size: 1024,
        flags: 0,
        internal_date: 1700000000,
        message_id: "<msg1@example.com>".into(),
        in_reply_to: "".into(),
        thread_id: "".into(),
        modseq: 1,
        user_address: "test@example.com".into(),
        importance_level: "normal".into(),
        importance_score: 0.0,
        is_bulk_sender: false,
        has_tracking_pixel: false,
        new_content: None,
    };
    overrides(&mut msg);
    msg
}

// -- message_matches_criteria --

#[test]
fn matches_all() {
    let msg = make_msg(|_| {});
    assert!(message_matches_criteria(&msg, &[SearchKey::All]));
}

#[test]
fn matches_seen_flag() {
    let msg = make_msg(|m| m.flags = FLAG_SEEN);
    assert!(message_matches_criteria(&msg, &[SearchKey::Seen]));
    assert!(!message_matches_criteria(&msg, &[SearchKey::Unseen]));
}

#[test]
fn matches_unseen_no_flag() {
    let msg = make_msg(|_| {});
    assert!(message_matches_criteria(&msg, &[SearchKey::Unseen]));
    assert!(!message_matches_criteria(&msg, &[SearchKey::Seen]));
}

#[test]
fn matches_flagged() {
    let flagged = make_msg(|m| m.flags = FLAG_FLAGGED);
    let unflagged = make_msg(|_| {});
    assert!(message_matches_criteria(&flagged, &[SearchKey::Flagged]));
    assert!(message_matches_criteria(&unflagged, &[SearchKey::Unflagged]));
    assert!(!message_matches_criteria(&flagged, &[SearchKey::Unflagged]));
    assert!(!message_matches_criteria(&unflagged, &[SearchKey::Flagged]));
}

#[test]
fn matches_answered() {
    let answered = make_msg(|m| m.flags = FLAG_ANSWERED);
    let unanswered = make_msg(|_| {});
    assert!(message_matches_criteria(&answered, &[SearchKey::Answered]));
    assert!(message_matches_criteria(&unanswered, &[SearchKey::Unanswered]));
}

#[test]
fn matches_deleted() {
    let deleted = make_msg(|m| m.flags = FLAG_DELETED);
    let not_deleted = make_msg(|_| {});
    assert!(message_matches_criteria(&deleted, &[SearchKey::Deleted]));
    assert!(message_matches_criteria(&not_deleted, &[SearchKey::Undeleted]));
}

#[test]
fn matches_draft() {
    let draft = make_msg(|m| m.flags = FLAG_DRAFT);
    let not_draft = make_msg(|_| {});
    assert!(message_matches_criteria(&draft, &[SearchKey::Draft]));
    assert!(message_matches_criteria(&not_draft, &[SearchKey::Undraft]));
}

#[test]
fn matches_recent() {
    let recent = make_msg(|m| m.flags = FLAG_RECENT);
    assert!(message_matches_criteria(&recent, &[SearchKey::Recent]));
    assert!(!message_matches_criteria(&make_msg(|_| {}), &[SearchKey::Recent]));
}

#[test]
fn matches_from_case_insensitive() {
    let msg = make_msg(|m| m.sender = "Alice@Example.COM".into());
    assert!(message_matches_criteria(&msg, &[SearchKey::From("alice".into())]));
    assert!(message_matches_criteria(&msg, &[SearchKey::From("ALICE".into())]));
    assert!(!message_matches_criteria(&msg, &[SearchKey::From("bob".into())]));
}

#[test]
fn matches_to_case_insensitive() {
    let msg = make_msg(|m| m.recipients = "Bob@Example.COM".into());
    assert!(message_matches_criteria(&msg, &[SearchKey::To("bob".into())]));
    assert!(!message_matches_criteria(&msg, &[SearchKey::To("alice".into())]));
}

#[test]
fn matches_subject_case_insensitive() {
    let msg = make_msg(|m| m.subject = "Meeting Tomorrow".into());
    assert!(message_matches_criteria(&msg, &[SearchKey::Subject("meeting".into())]));
    assert!(message_matches_criteria(&msg, &[SearchKey::Subject("TOMORROW".into())]));
    assert!(!message_matches_criteria(&msg, &[SearchKey::Subject("yesterday".into())]));
}

#[test]
fn matches_text_searches_multiple_fields() {
    let msg = make_msg(|m| {
        m.sender = "alice@example.com".into();
        m.recipients = "bob@example.com".into();
        m.subject = "Important Update".into();
    });
    assert!(message_matches_criteria(&msg, &[SearchKey::Text("alice".into())]));
    assert!(message_matches_criteria(&msg, &[SearchKey::Text("bob".into())]));
    assert!(message_matches_criteria(&msg, &[SearchKey::Text("important".into())]));
    assert!(!message_matches_criteria(&msg, &[SearchKey::Text("charlie".into())]));
}

#[test]
fn matches_since_before_on() {
    let msg = make_msg(|m| m.date = 1700000000);
    assert!(message_matches_criteria(&msg, &[SearchKey::Since(1699999999)]));
    assert!(message_matches_criteria(&msg, &[SearchKey::Since(1700000000)]));
    assert!(!message_matches_criteria(&msg, &[SearchKey::Since(1700000001)]));

    assert!(message_matches_criteria(&msg, &[SearchKey::Before(1700000001)]));
    assert!(!message_matches_criteria(&msg, &[SearchKey::Before(1700000000)]));

    assert!(message_matches_criteria(&msg, &[SearchKey::On(1700000000)]));
    assert!(!message_matches_criteria(&msg, &[SearchKey::On(1700000000 + 86400)]));
}

#[test]
fn matches_multiple_criteria_all_must_match() {
    let msg = make_msg(|m| {
        m.flags = FLAG_SEEN;
        m.sender = "alice@example.com".into();
    });
    assert!(message_matches_criteria(
        &msg,
        &[SearchKey::Seen, SearchKey::From("alice".into())]
    ));
    assert!(!message_matches_criteria(
        &msg,
        &[SearchKey::Seen, SearchKey::From("bob".into())]
    ));
    assert!(!message_matches_criteria(
        &msg,
        &[SearchKey::Unseen, SearchKey::From("alice".into())]
    ));
}

#[test]
fn matches_empty_criteria_returns_true() {
    let msg = make_msg(|_| {});
    assert!(message_matches_criteria(&msg, &[]));
}

#[test]
fn matches_uid_search() {
    let msg = make_msg(|m| m.uid = 42);
    assert!(message_matches_criteria(&msg, &[SearchKey::Uid("42".into())]));
    assert!(message_matches_criteria(&msg, &[SearchKey::Uid("40:45".into())]));
    assert!(!message_matches_criteria(&msg, &[SearchKey::Uid("1:10".into())]));
}

