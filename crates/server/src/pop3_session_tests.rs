//! Tests for `pop3_session` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

fn make_transaction_session(messages: Vec<MessageEntry>) -> Pop3Session {
    let pool = sqlx::PgPool::connect_lazy("postgres://user:pass@localhost/db").unwrap();
    Pop3Session {
        mailbox_store: Arc::new(PgMailboxStore::new(pool)),
        users: Arc::new(UserStore::empty()),
        state: Pop3State::Transaction {
            username: "test@example.com".into(),
            messages,
        },
        maildir_root: String::new(),
        pending_user: None,
        auth_guard: None,
        peer_addr: None,
        domain_store: None,
        ldap_config: None,
    }
}

#[tokio::test]
async fn stat_in_transaction() {
    let session = make_transaction_session(vec![
        MessageEntry { uid: 1, maildir_id: "a".into(), size: 100, deleted: false },
        MessageEntry { uid: 2, maildir_id: "b".into(), size: 200, deleted: true },
    ]);
    let resp = session.handle_stat();
    assert_eq!(resp[0], "+OK 1 100\r\n");
}

#[tokio::test]
async fn list_all() {
    let session = make_transaction_session(vec![
        MessageEntry { uid: 1, maildir_id: "a".into(), size: 100, deleted: false },
        MessageEntry { uid: 2, maildir_id: "b".into(), size: 200, deleted: false },
    ]);
    let resp = session.handle_list("");
    assert!(resp[0].starts_with("+OK 2 messages"));
    assert_eq!(resp[1], "1 100\r\n");
    assert_eq!(resp[2], "2 200\r\n");
    assert_eq!(resp[3], ".\r\n");
}

#[tokio::test]
async fn list_single() {
    let session = make_transaction_session(vec![
        MessageEntry { uid: 1, maildir_id: "a".into(), size: 100, deleted: false },
    ]);
    let resp = session.handle_list("1");
    assert_eq!(resp[0], "+OK 1 100\r\n");
}

#[tokio::test]
async fn list_deleted_message() {
    let session = make_transaction_session(vec![
        MessageEntry { uid: 1, maildir_id: "a".into(), size: 100, deleted: true },
    ]);
    let resp = session.handle_list("1");
    assert!(resp[0].starts_with("-ERR"));
}

#[tokio::test]
async fn list_out_of_range() {
    let session = make_transaction_session(vec![]);
    let resp = session.handle_list("1");
    assert!(resp[0].starts_with("-ERR"));
}

#[tokio::test]
async fn dele_and_rset() {
    let mut session = make_transaction_session(vec![
        MessageEntry { uid: 1, maildir_id: "a".into(), size: 100, deleted: false },
    ]);
    let resp = session.handle_dele("1");
    assert!(resp[0].starts_with("+OK"));
    let (count, _) = session.stat_values();
    assert_eq!(count, 0);

    let resp = session.handle_rset();
    assert!(resp[0].starts_with("+OK"));
    let (count, _) = session.stat_values();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn dele_already_deleted() {
    let mut session = make_transaction_session(vec![
        MessageEntry { uid: 1, maildir_id: "a".into(), size: 100, deleted: true },
    ]);
    let resp = session.handle_dele("1");
    assert!(resp[0].starts_with("-ERR"));
}

#[tokio::test]
async fn uidl_all() {
    let session = make_transaction_session(vec![
        MessageEntry { uid: 42, maildir_id: "a".into(), size: 100, deleted: false },
    ]);
    let resp = session.handle_uidl("");
    assert_eq!(resp[0], "+OK\r\n");
    assert_eq!(resp[1], "1 42\r\n");
    assert_eq!(resp[2], ".\r\n");
}

#[tokio::test]
async fn uidl_single() {
    let session = make_transaction_session(vec![
        MessageEntry { uid: 42, maildir_id: "a".into(), size: 100, deleted: false },
    ]);
    let resp = session.handle_uidl("1");
    assert_eq!(resp[0], "+OK 1 42\r\n");
}

#[tokio::test]
async fn capa_response() {
    let session = make_transaction_session(vec![]);
    let resp = session.handle_capa();
    let joined = resp.join("");
    assert!(joined.contains("USER"));
    assert!(joined.contains("UIDL"));
    assert!(joined.contains("TOP"));
    assert!(joined.ends_with(".\r\n"));
}
