//! Integration + unit tests for `ImapSession` and its sibling
//! helpers. Originally lived inline at the bottom of
//! `imap_session.rs`; moved here when the file split into the
//! `imap_session/` directory.
//!
//! Integration tests (`#[ignore]`) require a live PostgreSQL via
//! `MAILRS_PG_URL`. Unit tests run unconditionally and exercise
//! the pure-function helpers (matcher, IMAP wire formatters,
//! flag round-trip, MIME splitter, BODYSTRUCTURE builder).

use std::sync::Arc;

use mailrs_imap_format::{
    build_bodystructure, escape_imap_str, escape_imap_string, extract_body_section,
    extract_header_fields, extract_header_section, find_line_offset, format_addr_list,
    format_imap_address, format_imap_flags, format_internal_date, parse_generic_body_sections,
    parse_header_fields_request, parse_imap_flags, quote_or_nil, split_mime_parts,
    trim_part_trailing_newline,
};
use mailrs_imap_proto::SearchKey;
use mailrs_mailbox::{
    PgMailboxStore, FLAG_ANSWERED, FLAG_DELETED, FLAG_DRAFT, FLAG_FLAGGED, FLAG_RECENT, FLAG_SEEN,
};

use super::search::message_matches_criteria;
use super::{imap_greeting, strs_to_bytes, HandleResult, ImapSession};
use crate::users::UserStore;

/// requires MAILRS_PG_URL env var pointing to a test database
async fn test_session() -> ImapSession {
    let url =
        std::env::var("MAILRS_PG_URL").expect("MAILRS_PG_URL required for integration tests");
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    let store = Arc::new(PgMailboxStore::new(pool));
    let users = Arc::new(UserStore::from_plain_passwords(vec![(
        "alice@example.com".into(),
        "password123".into(),
    )]));
    ImapSession::new(store, users)
}

/// extract responses from HandleResult as strings, panicking on NeedLiteral/EnterIdle
fn responses(result: HandleResult) -> Vec<String> {
    match result {
        HandleResult::Responses(r) => r
            .into_iter()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .collect(),
        HandleResult::NeedLiteral { .. } => panic!("unexpected NeedLiteral"),
        HandleResult::EnterIdle { .. } => panic!("unexpected EnterIdle"),
    }
}

#[tokio::test]
#[ignore]
async fn login_success() {
    let mut session = test_session().await;
    let resp = responses(
        session
            .handle_line("a001 LOGIN alice@example.com password123")
            .await,
    );
    assert!(resp.last().unwrap().contains("OK"));
    assert!(resp.last().unwrap().contains("LOGIN completed"));
}

#[tokio::test]
#[ignore]
async fn login_wrong_password() {
    let mut session = test_session().await;
    let resp = responses(
        session
            .handle_line("a001 LOGIN alice@example.com wrongpass")
            .await,
    );
    assert!(resp.last().unwrap().contains("NO"));
}

#[tokio::test]
#[ignore]
async fn select_inbox() {
    let mut session = test_session().await;
    session
        .handle_line("a001 LOGIN alice@example.com password123")
        .await;
    let resp = responses(session.handle_line("a002 SELECT INBOX").await);
    let joined = resp.join("");
    assert!(joined.contains("FLAGS"));
    assert!(joined.contains("EXISTS"));
    assert!(joined.contains("UIDVALIDITY"));
    assert!(resp.last().unwrap().contains("OK"));
}

#[tokio::test]
#[ignore]
async fn select_not_authenticated() {
    let mut session = test_session().await;
    let resp = responses(session.handle_line("a002 SELECT INBOX").await);
    assert!(resp.last().unwrap().contains("NO"));
}

#[tokio::test]
#[ignore]
async fn fetch_flags() {
    let mut session = test_session().await;
    session
        .handle_line("a001 LOGIN alice@example.com password123")
        .await;
    session.handle_line("a002 SELECT INBOX").await;

    // index a message
    session
        .mailbox_store
        .index_message(
            "alice@example.com",
            "INBOX",
            "msg001",
            "sender@test.com",
            "alice@example.com",
            "Test Subject",
            1024,
            1700000000,
            "",
            "",
            "",
        )
        .await
        .unwrap();

    let resp = responses(session.handle_line("a003 FETCH 1 FLAGS").await);
    let joined = resp.join("");
    assert!(joined.contains("FETCH"));
    assert!(resp.last().unwrap().contains("OK"));
}

#[tokio::test]
#[ignore]
async fn store_seen_flag() {
    let mut session = test_session().await;
    session
        .handle_line("a001 LOGIN alice@example.com password123")
        .await;
    session.handle_line("a002 SELECT INBOX").await;

    session
        .mailbox_store
        .index_message(
            "alice@example.com",
            "INBOX",
            "msg001",
            "",
            "",
            "",
            100,
            1000,
            "",
            "",
            "",
        )
        .await
        .unwrap();

    let resp = responses(session.handle_line("a003 STORE 1 +FLAGS (\\Seen)").await);
    let joined = resp.join("");
    assert!(joined.contains("\\Seen"));
    assert!(resp.last().unwrap().contains("OK"));

    // verify flag was persisted
    let mb = session
        .mailbox_store
        .get_mailbox("alice@example.com", "INBOX")
        .await
        .unwrap()
        .unwrap();
    let msg = session
        .mailbox_store
        .get_message(mb.id, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(msg.flags & FLAG_SEEN, FLAG_SEEN);
}

#[tokio::test]
#[ignore]
async fn list_default_mailboxes() {
    let mut session = test_session().await;
    session
        .handle_line("a001 LOGIN alice@example.com password123")
        .await;
    let resp = responses(session.handle_line("a002 LIST \"\" \"*\"").await);
    let joined = resp.join("");
    assert!(joined.contains("INBOX"));
    assert!(joined.contains("Sent"));
    assert!(joined.contains("Drafts"));
    assert!(joined.contains("Trash"));
    assert!(resp.last().unwrap().contains("OK"));
}

#[tokio::test]
#[ignore]
async fn capability_response() {
    let mut session = test_session().await;
    let resp = responses(session.handle_line("a001 CAPABILITY").await);
    let joined = resp.join("");
    assert!(joined.contains("IMAP4rev1"));
    assert!(resp.last().unwrap().contains("OK"));
}

#[tokio::test]
#[ignore]
async fn logout() {
    let mut session = test_session().await;
    session
        .handle_line("a001 LOGIN alice@example.com password123")
        .await;
    let resp = responses(session.handle_line("a002 LOGOUT").await);
    let joined = resp.join("");
    assert!(joined.contains("BYE"));
    assert!(resp.last().unwrap().contains("OK"));
}

#[test]
fn format_imap_flags_all() {
    let flags =
        FLAG_SEEN | FLAG_ANSWERED | FLAG_FLAGGED | FLAG_DELETED | FLAG_DRAFT | FLAG_RECENT;
    let s = format_imap_flags(flags);
    assert!(s.contains("\\Seen"));
    assert!(s.contains("\\Answered"));
    assert!(s.contains("\\Flagged"));
    assert!(s.contains("\\Deleted"));
    assert!(s.contains("\\Draft"));
    assert!(s.contains("\\Recent"));
}

#[test]
fn parse_imap_flags_parenthesized() {
    let bits = parse_imap_flags("(\\Seen \\Flagged)");
    assert_eq!(bits, FLAG_SEEN | FLAG_FLAGGED);
}

#[tokio::test]
#[ignore]
async fn expunge_test() {
    let mut session = test_session().await;
    session
        .handle_line("a001 LOGIN alice@example.com password123")
        .await;
    session.handle_line("a002 SELECT INBOX").await;

    session
        .mailbox_store
        .index_message(
            "alice@example.com",
            "INBOX",
            "msg001",
            "",
            "",
            "",
            100,
            1000,
            "",
            "",
            "",
        )
        .await
        .unwrap();
    session
        .mailbox_store
        .index_message(
            "alice@example.com",
            "INBOX",
            "msg002",
            "",
            "",
            "",
            200,
            2000,
            "",
            "",
            "",
        )
        .await
        .unwrap();

    // mark first as deleted
    session.handle_line("a003 STORE 1 +FLAGS (\\Deleted)").await;

    let resp = responses(session.handle_line("a004 EXPUNGE").await);
    let joined = resp.join("");
    assert!(joined.contains("EXPUNGE"));
    assert!(resp.last().unwrap().contains("OK"));
}

#[tokio::test]
#[ignore]
async fn append_needs_literal() {
    let mut session = test_session().await;
    session
        .handle_line("a001 LOGIN alice@example.com password123")
        .await;

    let result = session.handle_line("a002 APPEND INBOX {100}").await;
    match result {
        HandleResult::NeedLiteral { continuation, size } => {
            assert!(continuation.starts_with(b"+"));
            assert_eq!(size, 100);
        }
        _ => panic!("expected NeedLiteral"),
    }
}

#[tokio::test]
#[ignore]
async fn append_not_authenticated() {
    let mut session = test_session().await;
    let resp = responses(session.handle_line("a002 APPEND INBOX {100}").await);
    assert!(resp.last().unwrap().contains("NO"));
}

#[tokio::test]
#[ignore]
async fn uid_fetch() {
    let mut session = test_session().await;
    session
        .handle_line("a001 LOGIN alice@example.com password123")
        .await;
    session.handle_line("a002 SELECT INBOX").await;

    session
        .mailbox_store
        .index_message(
            "alice@example.com",
            "INBOX",
            "msg001",
            "",
            "",
            "",
            100,
            1000,
            "",
            "",
            "",
        )
        .await
        .unwrap();

    let resp = responses(session.handle_line("a003 UID FETCH 1 FLAGS").await);
    let joined = resp.join("");
    eprintln!("UID FETCH response: {:?}", resp);
    assert!(joined.contains("UID 1"));
    assert!(joined.contains("FETCH"));
    assert!(resp.last().unwrap().contains("OK"));
}

#[test]
fn format_imap_address_with_name() {
    let result = format_imap_address("Alice <alice@example.com>");
    assert_eq!(result, "((\"Alice\" NIL \"alice\" \"example.com\"))");
}

#[test]
fn format_imap_address_plain() {
    let result = format_imap_address("user@host.com");
    assert_eq!(result, "((NIL NIL \"user\" \"host.com\"))");
}

#[test]
fn format_imap_address_empty() {
    assert_eq!(format_imap_address(""), "NIL");
}

#[test]
fn format_addr_list_multiple() {
    let result = format_addr_list("alice@a.com, bob@b.com");
    assert!(result.contains("\"alice\""));
    assert!(result.contains("\"bob\""));
    assert!(result.starts_with('('));
    assert!(result.ends_with(')'));
}

#[tokio::test]
#[ignore]
async fn idle_not_authenticated() {
    let mut session = test_session().await;
    assert!(session.idle_user().is_none());

    let resp = responses(session.handle_line("a001 IDLE").await);
    assert!(resp.last().unwrap().contains("NO"));
}

#[tokio::test]
#[ignore]
async fn idle_authenticated() {
    let mut session = test_session().await;
    session
        .handle_line("a001 LOGIN alice@example.com password123")
        .await;
    assert_eq!(session.idle_user(), Some("alice@example.com"));

    let result = session.handle_line("a002 IDLE").await;
    match result {
        HandleResult::EnterIdle { continuation, tag } => {
            let cont_str = String::from_utf8_lossy(&continuation);
            assert!(cont_str.contains("idling"));
            assert_eq!(tag, "a002");
        }
        _ => panic!("expected EnterIdle"),
    }
}

#[tokio::test]
#[ignore]
async fn idle_selected() {
    let mut session = test_session().await;
    session
        .handle_line("a001 LOGIN alice@example.com password123")
        .await;
    session.handle_line("a002 SELECT INBOX").await;
    assert_eq!(session.idle_user(), Some("alice@example.com"));
    assert!(session.selected_mailbox_id().is_some());
}

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

// -- format_imap_flags --

#[test]
fn format_flags_empty() {
    assert_eq!(format_imap_flags(0), "");
}

#[test]
fn format_flags_single() {
    assert_eq!(format_imap_flags(FLAG_SEEN), "\\Seen");
    assert_eq!(format_imap_flags(FLAG_DRAFT), "\\Draft");
}

#[test]
fn format_flags_multiple() {
    let s = format_imap_flags(FLAG_SEEN | FLAG_FLAGGED);
    assert_eq!(s, "\\Seen \\Flagged");
}

// -- parse_imap_flags --

#[test]
fn parse_flags_empty() {
    assert_eq!(parse_imap_flags(""), 0);
    assert_eq!(parse_imap_flags("()"), 0);
}

#[test]
fn parse_flags_without_parens() {
    assert_eq!(parse_imap_flags("\\Seen"), FLAG_SEEN);
}

#[test]
fn parse_flags_all() {
    let bits = parse_imap_flags("(\\Seen \\Answered \\Flagged \\Deleted \\Draft \\Recent)");
    assert_eq!(
        bits,
        FLAG_SEEN | FLAG_ANSWERED | FLAG_FLAGGED | FLAG_DELETED | FLAG_DRAFT | FLAG_RECENT
    );
}

#[test]
fn parse_flags_case_insensitive() {
    assert_eq!(parse_imap_flags("(\\seen \\FLAGGED)"), FLAG_SEEN | FLAG_FLAGGED);
}

#[test]
fn parse_flags_unknown_ignored() {
    assert_eq!(parse_imap_flags("(\\Seen \\CustomFlag)"), FLAG_SEEN);
}

// -- format_imap_flags / parse_imap_flags roundtrip --

#[test]
fn flags_roundtrip() {
    let original = FLAG_SEEN | FLAG_ANSWERED | FLAG_FLAGGED | FLAG_DELETED | FLAG_DRAFT | FLAG_RECENT;
    let formatted = format_imap_flags(original);
    let parsed = parse_imap_flags(&format!("({})", formatted));
    assert_eq!(parsed, original);
}

// -- escape_imap_string --

#[test]
fn escape_plain_string() {
    assert_eq!(escape_imap_string("hello"), "hello");
}

#[test]
fn escape_quotes_and_backslashes() {
    assert_eq!(escape_imap_string(r#"say "hi""#), r#"say \"hi\""#);
    assert_eq!(escape_imap_string(r"path\to"), r"path\\to");
}

// -- quote_or_nil --

#[test]
fn quote_or_nil_empty() {
    assert_eq!(quote_or_nil(""), "NIL");
}

#[test]
fn quote_or_nil_non_empty() {
    assert_eq!(quote_or_nil("hello"), "\"hello\"");
}

#[test]
fn quote_or_nil_special_chars() {
    assert_eq!(quote_or_nil(r#"a"b"#), r#""a\"b""#);
}

// -- format_imap_address --

#[test]
fn address_no_at() {
    assert_eq!(format_imap_address("localonly"), "((NIL NIL \"localonly\" \"\"))");
}

#[test]
fn address_with_quoted_name() {
    let result = format_imap_address("\"Bob Smith\" <bob@example.com>");
    assert_eq!(result, "((\"Bob Smith\" NIL \"bob\" \"example.com\"))");
}

#[test]
fn address_name_without_quotes() {
    let result = format_imap_address("Bob Smith <bob@example.com>");
    assert_eq!(result, "((\"Bob Smith\" NIL \"bob\" \"example.com\"))");
}

#[test]
fn address_angle_bracket_no_name() {
    let result = format_imap_address("<alice@example.com>");
    assert_eq!(result, "((NIL NIL \"alice\" \"example.com\"))");
}

// -- format_addr_list --

#[test]
fn addr_list_empty() {
    assert_eq!(format_addr_list(""), "NIL");
    assert_eq!(format_addr_list("  "), "NIL");
}

#[test]
fn addr_list_single() {
    let result = format_addr_list("alice@example.com");
    assert_eq!(result, "((NIL NIL \"alice\" \"example.com\"))");
}

#[test]
fn addr_list_with_names() {
    let result = format_addr_list("Alice <alice@a.com>, Bob <bob@b.com>");
    assert!(result.starts_with('('));
    assert!(result.ends_with(')'));
    assert!(result.contains("\"Alice\""));
    assert!(result.contains("\"Bob\""));
}

// -- imap_greeting --

#[test]
fn greeting_format() {
    let g = imap_greeting("mail.example.com");
    let s = String::from_utf8(g).unwrap();
    assert!(s.starts_with("* OK"));
    assert!(s.contains("mail.example.com"));
    assert!(s.contains("IMAP4rev1"));
    assert!(s.ends_with("\r\n"));
}

// -- strs_to_bytes --

#[test]
fn strs_to_bytes_empty() {
    let result = strs_to_bytes(vec![]);
    assert!(result.is_empty());
}

#[test]
fn strs_to_bytes_converts() {
    let result = strs_to_bytes(vec!["hello".into(), "world".into()]);
    assert_eq!(result, vec![b"hello".to_vec(), b"world".to_vec()]);
}

// -- format_internal_date --

#[test]
fn format_internal_date_known_timestamp() {
    let result = format_internal_date(0);
    // unix epoch: 1970-01-01
    assert!(result.contains("1970"));
    assert!(result.contains("Jan"));
}

#[test]
fn format_internal_date_recent() {
    let result = format_internal_date(1700000000);
    // 2023-11-14 in UTC
    assert!(result.contains("2023"));
    assert!(result.contains("Nov"));
}

// -- extract_header_section --

#[test]
fn extract_header_crlf() {
    let data = b"From: alice\r\nTo: bob\r\n\r\nBody here";
    let header = extract_header_section(data);
    assert_eq!(header, b"From: alice\r\nTo: bob\r\n\r\n");
}

#[test]
fn extract_header_lf_only() {
    let data = b"From: alice\nTo: bob\n\nBody here";
    let header = extract_header_section(data);
    assert_eq!(header, b"From: alice\nTo: bob\n\n");
}

#[test]
fn extract_header_no_separator() {
    let data = b"From: alice\r\nTo: bob";
    let header = extract_header_section(data);
    assert_eq!(header, data.to_vec());
}

// -- extract_body_section --

#[test]
fn extract_body_crlf() {
    let data = b"From: alice\r\n\r\nBody content";
    let body = extract_body_section(data);
    assert_eq!(body, b"Body content");
}

#[test]
fn extract_body_lf_only() {
    let data = b"From: alice\n\nBody content";
    let body = extract_body_section(data);
    assert_eq!(body, b"Body content");
}

#[test]
fn extract_body_no_separator() {
    let data = b"From: alice";
    let body = extract_body_section(data);
    assert!(body.is_empty());
}

// -- extract_header_fields --

#[test]
fn extract_specific_headers() {
    let data = b"From: alice@example.com\r\nTo: bob@example.com\r\nSubject: Test\r\nDate: Mon, 1 Jan 2024\r\n\r\nBody";
    let fields = vec!["FROM".into(), "SUBJECT".into()];
    let result = extract_header_fields(data, &fields);
    let s = String::from_utf8(result).unwrap();
    assert!(s.contains("From: alice@example.com"));
    assert!(s.contains("Subject: Test"));
    assert!(!s.contains("To:"));
    assert!(!s.contains("Date:"));
}

#[test]
fn extract_header_fields_with_continuation() {
    let data = b"Subject: This is a\r\n very long subject\r\nFrom: alice\r\n\r\nBody";
    let fields = vec!["SUBJECT".into()];
    let result = extract_header_fields(data, &fields);
    let s = String::from_utf8(result).unwrap();
    assert!(s.contains("Subject: This is a"));
    assert!(s.contains("very long subject"));
    assert!(!s.contains("From:"));
}

// -- parse_header_fields_request --

#[test]
fn parse_header_fields_basic() {
    let input = "BODY[HEADER.FIELDS (FROM TO SUBJECT)]";
    let (fields, raw) = parse_header_fields_request(input).unwrap();
    assert_eq!(fields, vec!["FROM", "TO", "SUBJECT"]);
    assert_eq!(raw, "HEADER.FIELDS (FROM TO SUBJECT)");
}

#[test]
fn parse_header_fields_peek() {
    let input = "BODY.PEEK[HEADER.FIELDS (DATE FROM)]";
    let (fields, _raw) = parse_header_fields_request(input).unwrap();
    assert_eq!(fields, vec!["DATE", "FROM"]);
}

#[test]
fn parse_header_fields_no_match() {
    assert!(parse_header_fields_request("BODY[]").is_none());
    assert!(parse_header_fields_request("FLAGS").is_none());
}

// -- parse_generic_body_sections --

#[test]
fn parse_body_section_numeric() {
    let sections = parse_generic_body_sections("BODY[1]");
    assert_eq!(sections, vec!["1"]);
}

#[test]
fn parse_body_section_nested() {
    let sections = parse_generic_body_sections("BODY[1.1] BODY[2]");
    assert_eq!(sections, vec!["1.1", "2"]);
}

#[test]
fn parse_body_section_peek() {
    let sections = parse_generic_body_sections("BODY.PEEK[1.MIME]");
    assert_eq!(sections, vec!["1.MIME"]);
}

#[test]
fn parse_body_section_skips_header_text() {
    let sections = parse_generic_body_sections("BODY[HEADER] BODY[TEXT] BODY[HEADER.FIELDS (FROM)]");
    assert!(sections.is_empty());
}

#[test]
fn parse_body_section_empty() {
    let sections = parse_generic_body_sections("BODY[]");
    assert!(sections.is_empty());
}

#[test]
fn parse_body_section_deduplicates() {
    let sections = parse_generic_body_sections("BODY[1] BODY.PEEK[1]");
    assert_eq!(sections, vec!["1"]);
}

// -- find_line_offset --

#[test]
fn find_line_offset_first_line() {
    let data = b"line0\nline1\nline2\n";
    assert_eq!(find_line_offset(data,0), Some(0));
}

#[test]
fn find_line_offset_middle() {
    let data = b"line0\nline1\nline2\n";
    assert_eq!(find_line_offset(data,1), Some(6));
    assert_eq!(find_line_offset(data,2), Some(12));
}

#[test]
fn find_line_offset_past_end() {
    let data = b"line0\nline1\n";
    assert_eq!(find_line_offset(data,10), None);
}

// -- trim_part_trailing_newline --

#[test]
fn trim_trailing_crlf() {
    assert_eq!(trim_part_trailing_newline(b"data\r\n"), b"data");
}

#[test]
fn trim_trailing_lf() {
    assert_eq!(trim_part_trailing_newline(b"data\n"), b"data");
}

#[test]
fn trim_trailing_no_newline() {
    assert_eq!(trim_part_trailing_newline(b"data"), b"data");
}

#[test]
fn trim_trailing_empty() {
    assert_eq!(trim_part_trailing_newline(b""), b"");
}

// -- escape_imap_str (the second one) --

#[test]
fn escape_imap_str_basic() {
    assert_eq!(escape_imap_str("plain"), "plain");
    assert_eq!(escape_imap_str(r#"a"b\c"#), r#"a\"b\\c"#);
}

// -- split_mime_parts --

#[test]
fn split_mime_simple() {
    let body = b"--boundary\r\nContent-Type: text/plain\r\n\r\npart1\r\n--boundary\r\nContent-Type: text/html\r\n\r\npart2\r\n--boundary--\r\n";
    let parts = split_mime_parts(body, "boundary");
    assert_eq!(parts.len(), 2);
    assert!(String::from_utf8_lossy(parts[0]).contains("part1"));
    assert!(String::from_utf8_lossy(parts[1]).contains("part2"));
}

#[test]
fn split_mime_no_parts() {
    let body = b"no boundaries here";
    let parts = split_mime_parts(body, "boundary");
    assert!(parts.is_empty());
}

// -- build_bodystructure (basic smoke test) --

#[test]
fn build_bodystructure_text_plain() {
    let msg = b"Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 7bit\r\n\r\nHello world";
    let bs = build_bodystructure(msg);
    let upper = bs.to_uppercase();
    assert!(upper.contains("TEXT"));
    assert!(upper.contains("PLAIN"));
}

#[test]
fn build_bodystructure_multipart() {
    let msg = b"Content-Type: multipart/alternative; boundary=\"abc\"\r\n\r\n--abc\r\nContent-Type: text/plain\r\n\r\nplain\r\n--abc\r\nContent-Type: text/html\r\n\r\n<b>html</b>\r\n--abc--\r\n";
    let bs = build_bodystructure(msg);
    let upper = bs.to_uppercase();
    assert!(upper.contains("ALTERNATIVE"));
    assert!(upper.contains("PLAIN"));
    assert!(upper.contains("HTML"));
}
