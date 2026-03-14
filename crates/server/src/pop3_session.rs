use std::sync::Arc;

use mailrs_mailbox::MailboxStore;

use crate::domain_store::DomainStore;
use crate::inbound::auth_guard::{AuthCheck, AuthGuard};
use crate::message_util;
use crate::users::UserStore;

/// POP3 session state
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
    mailbox_store: Arc<MailboxStore>,
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
    pub fn new(mailbox_store: Arc<MailboxStore>, users: Arc<UserStore>) -> Self {
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

    fn handle_capa(&self) -> Vec<String> {
        vec![
            "+OK capability list follows\r\n".into(),
            "USER\r\n".into(),
            "UIDL\r\n".into(),
            "TOP\r\n".into(),
            "RESP-CODES\r\n".into(),
            ".\r\n".into(),
        ]
    }

    fn handle_user(&mut self, arg: &str) -> Vec<String> {
        if !matches!(self.state, Pop3State::Authorization) {
            return vec!["-ERR already authenticated\r\n".into()];
        }
        if arg.is_empty() {
            return vec!["-ERR username required\r\n".into()];
        }
        self.pending_user = Some(arg.to_string());
        vec!["+OK\r\n".into()]
    }

    async fn handle_pass(&mut self, password: &str) -> Vec<String> {
        if !matches!(self.state, Pop3State::Authorization) {
            return vec!["-ERR already authenticated\r\n".into()];
        }
        let Some(ref username) = self.pending_user.take() else {
            return vec!["-ERR USER first\r\n".into()];
        };
        if password.is_empty() {
            return vec!["-ERR password required\r\n".into()];
        }

        // check auth guard
        if let (Some(ref guard), Some(ip)) = (&self.auth_guard, self.peer_addr) {
            if let AuthCheck::LockedOut { .. } = guard.check(ip, username) {
                return vec!["-ERR [IN-USE] too many failures, try later\r\n".into()];
            }
        }

        // try domain store (PG accounts) first, then users.toml, then LDAP
        let authenticated = if let Some(ref ds) = self.domain_store {
            match ds.get_account_with_hash(username).await {
                Ok(Some((account, hash))) => {
                    if !account.active {
                        false
                    } else if hash.starts_with("$argon2") {
                        let valid = UserStore::verify_hash(password, &hash);
                        if valid {
                            true
                        } else if let Some(ref ldap) = self.ldap_config {
                            ldap.authenticate(username, password).await
                        } else {
                            false
                        }
                    } else if hash == password {
                        true
                    } else if let Some(ref ldap) = self.ldap_config {
                        ldap.authenticate(username, password).await
                    } else {
                        false
                    }
                }
                _ => {
                    if self.users.verify(username, password) {
                        true
                    } else if let Some(ref ldap) = self.ldap_config {
                        ldap.authenticate(username, password).await
                    } else {
                        false
                    }
                }
            }
        } else if self.users.verify(username, password) {
            true
        } else if let Some(ref ldap) = self.ldap_config {
            ldap.authenticate(username, password).await
        } else {
            false
        };

        if !authenticated {
            if let (Some(ref guard), Some(ip)) = (&self.auth_guard, self.peer_addr) {
                guard.record_failure(ip, username);
            }
            return vec!["-ERR [AUTH] invalid credentials\r\n".into()];
        }

        if let (Some(ref guard), Some(ip)) = (&self.auth_guard, self.peer_addr) {
            guard.record_success(ip, username);
        }

        // load INBOX messages
        let _ = self.mailbox_store.ensure_default_mailboxes(username).await;
        let messages = self.load_inbox(username).await;

        self.state = Pop3State::Transaction {
            username: username.clone(),
            messages,
        };

        let (count, size) = self.stat_values();
        vec![format!("+OK {username} has {count} messages ({size} octets)\r\n")]
    }

    async fn load_inbox(&self, user: &str) -> Vec<MessageEntry> {
        let mailboxes = self
            .mailbox_store
            .list_mailboxes(user)
            .await
            .unwrap_or_default();

        let inbox = mailboxes.iter().find(|mb| mb.name == "INBOX");
        let Some(inbox) = inbox else {
            return Vec::new();
        };

        let messages = self
            .mailbox_store
            .list_messages(inbox.id, 0, 10000)
            .await
            .unwrap_or_default();

        messages
            .into_iter()
            .map(|m| MessageEntry {
                uid: m.uid,
                maildir_id: m.maildir_id,
                size: m.size,
                deleted: false,
            })
            .collect()
    }

    fn stat_values(&self) -> (usize, u64) {
        if let Pop3State::Transaction { ref messages, .. } = self.state {
            let active: Vec<&MessageEntry> = messages.iter().filter(|m| !m.deleted).collect();
            let size: u64 = active.iter().map(|m| m.size as u64).sum();
            (active.len(), size)
        } else {
            (0, 0)
        }
    }

    fn handle_stat(&self) -> Vec<String> {
        if !matches!(self.state, Pop3State::Transaction { .. }) {
            return vec!["-ERR not authenticated\r\n".into()];
        }
        let (count, size) = self.stat_values();
        vec![format!("+OK {count} {size}\r\n")]
    }

    fn handle_list(&self, arg: &str) -> Vec<String> {
        let Pop3State::Transaction { ref messages, .. } = self.state else {
            return vec!["-ERR not authenticated\r\n".into()];
        };

        if arg.is_empty() {
            // list all
            let (count, size) = self.stat_values();
            let mut resp = vec![format!("+OK {count} messages ({size} octets)\r\n")];
            for (i, m) in messages.iter().enumerate() {
                if !m.deleted {
                    resp.push(format!("{} {}\r\n", i + 1, m.size));
                }
            }
            resp.push(".\r\n".into());
            resp
        } else {
            // list specific message
            let Ok(num) = arg.trim().parse::<usize>() else {
                return vec!["-ERR invalid message number\r\n".into()];
            };
            if num == 0 || num > messages.len() {
                return vec!["-ERR no such message\r\n".into()];
            }
            let m = &messages[num - 1];
            if m.deleted {
                return vec!["-ERR message deleted\r\n".into()];
            }
            vec![format!("+OK {} {}\r\n", num, m.size)]
        }
    }

    fn handle_uidl(&self, arg: &str) -> Vec<String> {
        let Pop3State::Transaction { ref messages, .. } = self.state else {
            return vec!["-ERR not authenticated\r\n".into()];
        };

        if arg.is_empty() {
            let mut resp = vec!["+OK\r\n".into()];
            for (i, m) in messages.iter().enumerate() {
                if !m.deleted {
                    resp.push(format!("{} {}\r\n", i + 1, m.uid));
                }
            }
            resp.push(".\r\n".into());
            resp
        } else {
            let Ok(num) = arg.trim().parse::<usize>() else {
                return vec!["-ERR invalid message number\r\n".into()];
            };
            if num == 0 || num > messages.len() {
                return vec!["-ERR no such message\r\n".into()];
            }
            let m = &messages[num - 1];
            if m.deleted {
                return vec!["-ERR message deleted\r\n".into()];
            }
            vec![format!("+OK {} {}\r\n", num, m.uid)]
        }
    }

    async fn handle_retr(&self, arg: &str) -> Vec<String> {
        let Pop3State::Transaction {
            ref username,
            ref messages,
        } = self.state
        else {
            return vec!["-ERR not authenticated\r\n".into()];
        };

        let Ok(num) = arg.trim().parse::<usize>() else {
            return vec!["-ERR invalid message number\r\n".into()];
        };
        if num == 0 || num > messages.len() {
            return vec!["-ERR no such message\r\n".into()];
        }
        let m = &messages[num - 1];
        if m.deleted {
            return vec!["-ERR message deleted\r\n".into()];
        }

        let raw = message_util::read_message_raw(&self.maildir_root, username, &m.maildir_id);
        match raw {
            Some(data) => {
                let mut resp = vec![format!("+OK {} octets\r\n", data.len())];
                // byte-stuff lines starting with '.'
                let stuffed = mailrs_smtp_client::connection::dot_stuff(&data);
                resp.push(String::from_utf8_lossy(&stuffed).into_owned());
                if !stuffed.ends_with(b"\r\n") {
                    resp.push("\r\n".into());
                }
                resp.push(".\r\n".into());
                resp
            }
            None => vec!["-ERR message not found on disk\r\n".into()],
        }
    }

    async fn handle_top(&self, arg: &str) -> Vec<String> {
        let Pop3State::Transaction {
            ref username,
            ref messages,
        } = self.state
        else {
            return vec!["-ERR not authenticated\r\n".into()];
        };

        let parts: Vec<&str> = arg.trim().splitn(2, ' ').collect();
        if parts.len() != 2 {
            return vec!["-ERR usage: TOP msg lines\r\n".into()];
        }
        let Ok(num) = parts[0].parse::<usize>() else {
            return vec!["-ERR invalid message number\r\n".into()];
        };
        let Ok(lines) = parts[1].parse::<usize>() else {
            return vec!["-ERR invalid line count\r\n".into()];
        };
        if num == 0 || num > messages.len() {
            return vec!["-ERR no such message\r\n".into()];
        }
        let m = &messages[num - 1];
        if m.deleted {
            return vec!["-ERR message deleted\r\n".into()];
        }

        let raw = message_util::read_message_raw(&self.maildir_root, username, &m.maildir_id);
        match raw {
            Some(data) => {
                let text = String::from_utf8_lossy(&data);
                // split at blank line (end of headers)
                let (headers, body) = match text.find("\r\n\r\n") {
                    Some(pos) => (&text[..pos + 2], &text[pos + 4..]),
                    None => (text.as_ref(), ""),
                };

                let mut resp = vec!["+OK\r\n".into()];
                resp.push(format!("{}\r\n", headers));
                resp.push("\r\n".into());

                // add requested number of body lines
                for (i, line) in body.lines().enumerate() {
                    if i >= lines {
                        break;
                    }
                    if line.starts_with('.') {
                        resp.push(format!(".{line}\r\n"));
                    } else {
                        resp.push(format!("{line}\r\n"));
                    }
                }
                resp.push(".\r\n".into());
                resp
            }
            None => vec!["-ERR message not found on disk\r\n".into()],
        }
    }

    fn handle_dele(&mut self, arg: &str) -> Vec<String> {
        let Pop3State::Transaction { ref mut messages, .. } = self.state else {
            return vec!["-ERR not authenticated\r\n".into()];
        };

        let Ok(num) = arg.trim().parse::<usize>() else {
            return vec!["-ERR invalid message number\r\n".into()];
        };
        if num == 0 || num > messages.len() {
            return vec!["-ERR no such message\r\n".into()];
        }
        let m = &mut messages[num - 1];
        if m.deleted {
            return vec!["-ERR message already deleted\r\n".into()];
        }
        m.deleted = true;
        vec![format!("+OK message {num} deleted\r\n")]
    }

    fn handle_rset(&mut self) -> Vec<String> {
        if let Pop3State::Transaction { ref mut messages, .. } = self.state {
            for m in messages.iter_mut() {
                m.deleted = false;
            }
            let (count, size) = self.stat_values();
            vec![format!("+OK {count} messages ({size} octets)\r\n")]
        } else {
            vec!["-ERR not authenticated\r\n".into()]
        }
    }

    async fn handle_quit(&mut self) -> Vec<String> {
        // if in transaction state, actually delete marked messages
        if let Pop3State::Transaction {
            ref username,
            ref messages,
        } = self.state
        {
            let deleted_uids: Vec<u32> = messages
                .iter()
                .filter(|m| m.deleted)
                .map(|m| m.uid)
                .collect();

            if !deleted_uids.is_empty() {
                let mailboxes = self
                    .mailbox_store
                    .list_mailboxes(username)
                    .await
                    .unwrap_or_default();
                if let Some(inbox) = mailboxes.iter().find(|mb| mb.name == "INBOX") {
                    // mark messages with \Deleted flag
                    for uid in &deleted_uids {
                        let _ = self
                            .mailbox_store
                            .add_flags(inbox.id, *uid, mailrs_mailbox::FLAG_DELETED)
                            .await;
                    }
                    // expunge (permanently remove flagged messages)
                    let _ = self.mailbox_store.expunge(inbox.id).await;
                }
            }
        }
        self.state = Pop3State::Authorization;
        vec!["+OK bye\r\n".into()]
    }

    /// returns true if the session should be closed (QUIT received)
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
            mailbox_store: Arc::new(MailboxStore::new(pool)),
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
