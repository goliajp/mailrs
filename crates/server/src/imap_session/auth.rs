//! IMAP authentication & session-lifecycle handlers:
//! LOGIN, LOGOUT, CAPABILITY, NAMESPACE, ENABLE.
//!
//! These verbs gate access to every other handler — LOGIN
//! transitions the session into `Authenticated`, LOGOUT back to
//! `NotAuthenticated`. CAPABILITY / NAMESPACE / ENABLE work in
//! any state by design (CAPABILITY pre-auth so clients can see
//! AUTH=PLAIN advertised; ENABLE post-auth per RFC 5161).

use mailrs_imap_proto::{format_bad, format_bye, format_capability, format_no, format_ok};

use crate::inbound::auth_guard::{AuthCheck, unix_now};

use super::{ImapSession, ImapState};

impl ImapSession {
    pub(super) fn handle_capability(&self, tag: &str) -> Vec<String> {
        vec![
            format_capability(&[
                "IMAP4rev1",
                "AUTH=PLAIN",
                "IDLE",
                "QUOTA",
                "CONDSTORE",
                "SPECIAL-USE",
                "NAMESPACE",
                "SORT",
                "ENABLE",
                "UNSELECT",
            ]),
            format_ok(tag, "CAPABILITY completed"),
        ]
    }

    pub(super) fn handle_namespace(&self, tag: &str) -> Vec<String> {
        vec![
            "* NAMESPACE ((\"\" \"/\")) NIL NIL\r\n".to_string(),
            format_ok(tag, "NAMESPACE completed"),
        ]
    }

    pub(super) async fn handle_login(
        &mut self,
        tag: &str,
        username: &str,
        password: &str,
    ) -> Vec<String> {
        if matches!(
            self.state,
            ImapState::Authenticated { .. } | ImapState::Selected { .. }
        ) {
            return vec![format_bad(tag, "already authenticated")];
        }

        if let (Some(guard), Some(ip)) = (&self.auth_guard, self.peer_addr)
            && let AuthCheck::LockedOut { remaining_secs } =
                guard.check(ip, username, unix_now()).await
        {
            return vec![format_no(
                tag,
                &format!("Too many auth failures, try again in {remaining_secs}s"),
            )];
        }

        // try users.toml first, then PG accounts table, then LDAP
        let ok = if self.users.verify(username, password) {
            true
        } else if let Some(ref ds) = self.domain_store {
            match ds.get_account_with_hash(username).await {
                Ok(Some((_account, hash))) => {
                    let valid = if hash.starts_with("$argon2") {
                        crate::users::UserStore::verify_hash(password, &hash)
                    } else {
                        hash == password
                    };
                    if valid {
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
                    if let Some(ref ldap) = self.ldap_config {
                        ldap.authenticate(username, password).await
                    } else {
                        false
                    }
                }
            }
        } else if let Some(ref ldap) = self.ldap_config {
            ldap.authenticate(username, password).await
        } else {
            // constant-time: do dummy argon2 work when no auth backend configured
            crate::users::dummy_verify(password);
            false
        };

        if ok {
            if let (Some(guard), Some(ip)) = (&self.auth_guard, self.peer_addr) {
                guard.record_success(ip, username).await;
            }
            let _ = self.mailbox_store.ensure_default_mailboxes(username).await;
            self.state = ImapState::Authenticated {
                username: username.to_string(),
            };
            vec![format_ok(tag, "LOGIN completed")]
        } else {
            if let (Some(guard), Some(ip)) = (&self.auth_guard, self.peer_addr) {
                guard.record_failure(ip, username, unix_now()).await;
            }
            vec![format_no(tag, "LOGIN failed")]
        }
    }

    pub(super) fn handle_logout(&mut self, tag: &str) -> Vec<String> {
        self.state = ImapState::NotAuthenticated;
        vec![
            format_bye("server logging out"),
            format_ok(tag, "LOGOUT completed"),
        ]
    }

    pub(super) fn handle_enable(&self, tag: &str, capabilities: &[String]) -> Vec<String> {
        // echo back the requested capabilities (RFC 5161)
        if matches!(self.state, ImapState::NotAuthenticated) {
            return vec![format_bad(tag, "ENABLE requires authentication")];
        }
        let caps = capabilities.join(" ");
        vec![
            format!("* ENABLED {caps}\r\n"),
            format_ok(tag, "ENABLE completed"),
        ]
    }
}
