//! Integration tests for `ImapSession` — require a running
//! PostgreSQL pointed at by `MAILRS_PG_URL`. All tests
//! `#[ignore]` so default runs skip them; CI / nightly
//! invocations opt in with `cargo test -- --ignored`.

use std::sync::Arc;

use mailrs_imap_format::{
    format_addr_list, format_imap_address, format_imap_flags, parse_imap_flags,
};
use mailrs_mailbox::{
    FLAG_ANSWERED, FLAG_DELETED, FLAG_DRAFT, FLAG_FLAGGED, FLAG_RECENT, FLAG_SEEN, PgMailboxStore,
};

use crate::imap_session::{HandleResult, ImapSession};
use crate::users::UserStore;

/// requires MAILRS_PG_URL env var pointing to a test database
async fn test_session() -> ImapSession {
    let url = std::env::var("MAILRS_PG_URL").expect("MAILRS_PG_URL required for integration tests");
    let pool = crate::pg::BackendPool::connect(&url).await.unwrap();
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
            .map(|b: Vec<u8>| String::from_utf8_lossy(&b).into_owned())
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
    let flags = FLAG_SEEN | FLAG_ANSWERED | FLAG_FLAGGED | FLAG_DELETED | FLAG_DRAFT | FLAG_RECENT;
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
