use std::sync::Arc;


use mailrs_mailbox::PgMailboxStore;

use crate::domain_store::DomainStore;
use crate::inbound::auth_guard::AuthGuard;
use crate::users::UserStore;


mod auth;
mod connection;
mod mutate;
mod query;
mod retrieve;

pub use connection::handle_connection;

enum Pop3State {
    Authorization,
    Transaction {
        username: String,
        /// (uid, maildir_id, size, deleted)
        messages: Vec<MessageEntry>,
    },
}

struct MessageEntry {
    uid: u32,
    maildir_id: String,
    size: u32,
    deleted: bool,
}

/// POP3 session handler (RFC 1939)
pub struct Pop3Session {
    mailbox_store: Arc<PgMailboxStore>,
    users: Arc<UserStore>,
    state: Pop3State,
    maildir_root: String,
    pending_user: Option<String>,
    auth_guard: Option<Arc<AuthGuard>>,
    peer_addr: Option<std::net::IpAddr>,
    domain_store: Option<Arc<DomainStore>>,
    ldap_config: Option<Arc<crate::ldap_auth::LdapConfig>>,
}

impl Pop3Session {
    pub fn new(mailbox_store: Arc<PgMailboxStore>, users: Arc<UserStore>) -> Self {
        Self {
            mailbox_store,
            users,
            state: Pop3State::Authorization,
            maildir_root: String::new(),
            pending_user: None,
            auth_guard: None,
            peer_addr: None,
            domain_store: None,
            ldap_config: None,
        }
    }

    pub fn with_maildir_root(mut self, root: &str) -> Self {
        self.maildir_root = root.to_string();
        self
    }

    pub fn with_auth_guard(mut self, guard: Arc<AuthGuard>, addr: std::net::IpAddr) -> Self {
        self.auth_guard = Some(guard);
        self.peer_addr = Some(addr);
        self
    }

    pub fn with_domain_store(mut self, ds: Arc<DomainStore>) -> Self {
        self.domain_store = Some(ds);
        self
    }

    pub fn with_ldap_config(mut self, config: Arc<crate::ldap_auth::LdapConfig>) -> Self {
        self.ldap_config = Some(config);
        self
    }

    pub fn greeting(&self) -> String {
        "+OK mailrs POP3 server ready\r\n".to_string()
    }

    /// handle a single POP3 command line, return response(s)
    pub async fn handle_line(&mut self, line: &str) -> Vec<String> {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let (cmd, arg) = match trimmed.split_once(' ') {
            Some((c, a)) => (c.to_uppercase(), a.to_string()),
            None => (trimmed.to_uppercase(), String::new()),
        };

        match cmd.as_str() {
            "QUIT" => self.handle_quit().await,
            "CAPA" => self.handle_capa(),
            "NOOP" => vec!["+OK\r\n".into()],
            "USER" => self.handle_user(&arg),
            "PASS" => self.handle_pass(&arg).await,
            "STAT" => self.handle_stat(),
            "LIST" => self.handle_list(&arg),
            "UIDL" => self.handle_uidl(&arg),
            "RETR" => self.handle_retr(&arg).await,
            "TOP" => self.handle_top(&arg).await,
            "DELE" => self.handle_dele(&arg),
            "RSET" => self.handle_rset(),
            _ => vec![format!("-ERR unknown command\r\n")],
        }
    }

    pub fn should_close(&self, responses: &[String]) -> bool {
        responses.last().is_some_and(|r| r.contains("bye"))
    }
}

#[cfg(test)]
mod tests {
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
}

