//! POP3 AUTHORIZATION-state commands: CAPA, USER, PASS.

use crate::inbound::auth_guard::AuthCheck;
use crate::users::UserStore;

use super::{Pop3Session, Pop3State};

impl Pop3Session {
    pub(super) fn handle_capa(&self) -> Vec<String> {
        vec![
            "+OK capability list follows\r\n".into(),
            "USER\r\n".into(),
            "UIDL\r\n".into(),
            "TOP\r\n".into(),
            "RESP-CODES\r\n".into(),
            ".\r\n".into(),
        ]
    }


    pub(super) fn handle_user(&mut self, arg: &str) -> Vec<String> {
        if !matches!(self.state, Pop3State::Authorization) {
            return vec!["-ERR already authenticated\r\n".into()];
        }
        if arg.is_empty() {
            return vec!["-ERR username required\r\n".into()];
        }
        self.pending_user = Some(arg.to_string());
        vec!["+OK\r\n".into()]
    }


    pub(super) async fn handle_pass(&mut self, password: &str) -> Vec<String> {
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
        if let (Some(guard), Some(ip)) = (&self.auth_guard, self.peer_addr)
            && let AuthCheck::LockedOut { .. } = guard.check(ip, username) {
                return vec!["-ERR [IN-USE] too many failures, try later\r\n".into()];
            }

        // try domain store (PG accounts) first, then users.toml, then LDAP
        let authenticated = if let Some(ref ds) = self.domain_store {
            match ds.get_account_with_hash(username).await {
                Ok(Some((account, hash))) => {
                    if !account.active {
                        false
                    } else if hash.is_empty() {
                        // accounts with no password hash cannot log in
                        if let Some(ref ldap) = self.ldap_config {
                            ldap.authenticate(username, password).await
                        } else {
                            false
                        }
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
                    // constant-time: do dummy argon2 work even when account not found
                    crate::users::dummy_verify(password);
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
            // constant-time: do dummy argon2 work when no auth backend configured
            crate::users::dummy_verify(password);
            false
        };

        if !authenticated {
            if let (Some(guard), Some(ip)) = (&self.auth_guard, self.peer_addr) {
                guard.record_failure(ip, username);
            }
            return vec!["-ERR [AUTH] invalid credentials\r\n".into()];
        }

        if let (Some(guard), Some(ip)) = (&self.auth_guard, self.peer_addr) {
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
}
