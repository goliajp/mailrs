//! IMAP mailbox-management handlers:
//! LIST, CREATE, DELETE, RENAME, SELECT, EXAMINE, CLOSE, UNSELECT,
//! STATUS, LSUB.
//!
//! All of these require Authenticated or Selected state.
//! SELECT/EXAMINE additionally transition state to Selected;
//! CLOSE/UNSELECT transition back to Authenticated.

use mailrs_imap_proto::{format_exists, format_flags, format_list, format_no, format_ok, format_recent};

use super::{ImapSession, ImapState};

impl ImapSession {
    pub(super) async fn handle_list(
        &self,
        tag: &str,
        _reference: &str,
        pattern: &str,
    ) -> Vec<String> {
        let username = match &self.state {
            ImapState::Authenticated { username } | ImapState::Selected { username, .. } => {
                username
            }
            ImapState::NotAuthenticated => {
                return vec![format_no(tag, "not authenticated")];
            }
        };

        let mailboxes = match self.mailbox_store.list_mailboxes(username).await {
            Ok(list) => list,
            Err(_) => return vec![format_no(tag, "LIST failed")],
        };

        let mut responses = Vec::new();
        for mb in &mailboxes {
            // simple pattern matching: "*" matches all, "%" matches non-hierarchy
            if pattern == "*" || pattern == "%" || mb.name == pattern {
                let special_use = match mb.name.as_str() {
                    "Sent" => " \\Sent",
                    "Drafts" => " \\Drafts",
                    "Trash" => " \\Trash",
                    "Junk" => " \\Junk",
                    _ => "",
                };
                let flags = format!("\\HasNoChildren{special_use}");
                responses.push(format_list(&flags, "/", &mb.name));
            }
        }
        responses.push(format_ok(tag, "LIST completed"));
        responses
    }

    pub(super) async fn handle_create(&self, tag: &str, mailbox: &str) -> Vec<String> {
        let username = match &self.state {
            ImapState::Authenticated { username } | ImapState::Selected { username, .. } => {
                username
            }
            ImapState::NotAuthenticated => {
                return vec![format_no(tag, "not authenticated")];
            }
        };

        match self.mailbox_store.create_mailbox(username, mailbox).await {
            Ok(_) => vec![format_ok(tag, "CREATE completed")],
            Err(e) => vec![format_no(tag, &format!("CREATE failed: {e}"))],
        }
    }

    pub(super) async fn handle_delete(&self, tag: &str, mailbox: &str) -> Vec<String> {
        let username = match &self.state {
            ImapState::Authenticated { username } | ImapState::Selected { username, .. } => {
                username
            }
            ImapState::NotAuthenticated => {
                return vec![format_no(tag, "not authenticated")];
            }
        };

        if mailbox.eq_ignore_ascii_case("INBOX") {
            return vec![format_no(tag, "cannot delete INBOX")];
        }

        match self.mailbox_store.delete_mailbox(username, mailbox).await {
            Ok(true) => vec![format_ok(tag, "DELETE completed")],
            Ok(false) => vec![format_no(tag, "mailbox not found")],
            Err(e) => vec![format_no(tag, &format!("DELETE failed: {e}"))],
        }
    }

    pub(super) async fn handle_rename(&self, tag: &str, from: &str, to: &str) -> Vec<String> {
        let username = match &self.state {
            ImapState::Authenticated { username } | ImapState::Selected { username, .. } => {
                username
            }
            ImapState::NotAuthenticated => {
                return vec![format_no(tag, "not authenticated")];
            }
        };

        if from.eq_ignore_ascii_case("INBOX") {
            return vec![format_no(tag, "cannot rename INBOX")];
        }

        match self
            .mailbox_store
            .rename_mailbox(username, from, to)
            .await
        {
            Ok(true) => vec![format_ok(tag, "RENAME completed")],
            Ok(false) => vec![format_no(tag, "mailbox not found")],
            Err(e) => vec![format_no(tag, &format!("RENAME failed: {e}"))],
        }
    }

    pub(super) async fn handle_select(&mut self, tag: &str, mailbox_name: &str) -> Vec<String> {
        let username = match &self.state {
            ImapState::Authenticated { username } | ImapState::Selected { username, .. } => {
                username.clone()
            }
            ImapState::NotAuthenticated => {
                return vec![format_no(tag, "not authenticated")];
            }
        };

        match self
            .mailbox_store
            .get_mailbox(&username, mailbox_name)
            .await
        {
            Ok(Some(mb)) => {
                let (total, unseen) = self
                    .mailbox_store
                    .mailbox_status(mb.id)
                    .await
                    .unwrap_or((0, 0));

                let mut responses = vec![
                    format_flags(&[
                        "\\Seen",
                        "\\Answered",
                        "\\Flagged",
                        "\\Deleted",
                        "\\Draft",
                        "\\Recent",
                    ]),
                    "* OK [PERMANENTFLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft \\*)] permanent flags\r\n".to_string(),
                    format_exists(total),
                    format_recent(0),
                    format!(
                        "* OK [UNSEEN {}] first unseen\r\n",
                        if unseen > 0 { 1 } else { 0 }
                    ),
                    format!("* OK [UIDVALIDITY {}] UIDs valid\r\n", mb.uidvalidity),
                    format!("* OK [UIDNEXT {}] predicted next UID\r\n", mb.uidnext),
                    format!(
                        "* OK [HIGHESTMODSEQ {}] highest modseq\r\n",
                        mb.highest_modseq
                    ),
                ];

                responses.push(format_ok(tag, "[READ-WRITE] SELECT completed"));

                self.state = ImapState::Selected {
                    username,
                    mailbox: mb,
                };
                responses
            }
            Ok(None) => vec![format_no(tag, "mailbox not found")],
            Err(_) => vec![format_no(tag, "SELECT failed")],
        }
    }

    pub(super) async fn handle_examine(&mut self, tag: &str, mailbox_name: &str) -> Vec<String> {
        // same as SELECT but read-only
        let username = match &self.state {
            ImapState::Authenticated { username } | ImapState::Selected { username, .. } => {
                username.clone()
            }
            ImapState::NotAuthenticated => {
                return vec![format_no(tag, "not authenticated")];
            }
        };

        match self
            .mailbox_store
            .get_mailbox(&username, mailbox_name)
            .await
        {
            Ok(Some(mb)) => {
                let (total, unseen) = self
                    .mailbox_store
                    .mailbox_status(mb.id)
                    .await
                    .unwrap_or((0, 0));

                let mut responses = vec![
                    format_flags(&[
                        "\\Seen",
                        "\\Answered",
                        "\\Flagged",
                        "\\Deleted",
                        "\\Draft",
                        "\\Recent",
                    ]),
                    "* OK [PERMANENTFLAGS ()] no permanent flags in read-only mode\r\n".to_string(),
                    format_exists(total),
                    format_recent(0),
                    format!(
                        "* OK [UNSEEN {}] first unseen\r\n",
                        if unseen > 0 { 1 } else { 0 }
                    ),
                    format!("* OK [UIDVALIDITY {}] UIDs valid\r\n", mb.uidvalidity),
                    format!("* OK [UIDNEXT {}] predicted next UID\r\n", mb.uidnext),
                    format!(
                        "* OK [HIGHESTMODSEQ {}] highest modseq\r\n",
                        mb.highest_modseq
                    ),
                ];

                responses.push(format_ok(tag, "[READ-ONLY] EXAMINE completed"));

                self.state = ImapState::Selected {
                    username,
                    mailbox: mb,
                };
                responses
            }
            Ok(None) => vec![format_no(tag, "mailbox not found")],
            Err(_) => vec![format_no(tag, "EXAMINE failed")],
        }
    }

    pub(super) async fn handle_close(&mut self, tag: &str) -> Vec<String> {
        // expunge deleted messages and return to authenticated state
        if let ImapState::Selected { mailbox, username } = &self.state {
            let _ = self.mailbox_store.expunge(mailbox.id).await;
            self.state = ImapState::Authenticated {
                username: username.clone(),
            };
        }
        vec![format_ok(tag, "CLOSE completed")]
    }

    pub(super) fn handle_unselect(&mut self, tag: &str) -> Vec<String> {
        // transition from Selected to Authenticated without expunging (RFC 3691)
        if let ImapState::Selected { ref username, .. } = self.state {
            self.state = ImapState::Authenticated {
                username: username.clone(),
            };
            vec![format_ok(tag, "UNSELECT completed")]
        } else {
            vec![format_no(tag, "not in selected state")]
        }
    }

    pub(super) async fn handle_status(
        &self,
        tag: &str,
        mailbox: &str,
        items: &str,
    ) -> Vec<String> {
        let username = match &self.state {
            ImapState::Authenticated { username } | ImapState::Selected { username, .. } => {
                username
            }
            ImapState::NotAuthenticated => return vec![format_no(tag, "not authenticated")],
        };

        // look up the mailbox by name to get accurate counts
        let mb = match self.mailbox_store.get_mailbox(username, mailbox).await {
            Ok(Some(mb)) => mb,
            Ok(None) => return vec![format_no(tag, "STATUS failed: mailbox not found")],
            Err(_) => return vec![format_no(tag, "STATUS failed")],
        };

        let (total, unseen) = self
            .mailbox_store
            .mailbox_status(mb.id)
            .await
            .unwrap_or((0, 0));

        let mut parts = Vec::new();
        let items_upper = items.to_uppercase();
        if items_upper.contains("MESSAGES") {
            parts.push(format!("MESSAGES {total}"));
        }
        if items_upper.contains("RECENT") {
            parts.push("RECENT 0".to_string());
        }
        if items_upper.contains("UIDNEXT") {
            parts.push(format!("UIDNEXT {}", mb.uidnext));
        }
        if items_upper.contains("UIDVALIDITY") {
            parts.push(format!("UIDVALIDITY {}", mb.uidvalidity));
        }
        if items_upper.contains("UNSEEN") {
            parts.push(format!("UNSEEN {unseen}"));
        }
        if items_upper.contains("HIGHESTMODSEQ") {
            parts.push(format!("HIGHESTMODSEQ {}", mb.highest_modseq));
        }

        vec![
            format!("* STATUS \"{}\" ({})\r\n", mailbox, parts.join(" ")),
            format_ok(tag, "STATUS completed"),
        ]
    }

    pub(super) async fn handle_lsub(
        &self,
        tag: &str,
        _reference: &str,
        _pattern: &str,
    ) -> Vec<String> {
        match &self.state {
            ImapState::Authenticated { .. } | ImapState::Selected { .. } => {
                vec![
                    "* LSUB () \"/\" \"INBOX\"".to_string(),
                    format_ok(tag, "LSUB completed"),
                ]
            }
            ImapState::NotAuthenticated => vec![format_no(tag, "not authenticated")],
        }
    }
}
