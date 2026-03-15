use std::sync::Arc;

use mailrs_imap_proto::{
    SearchKey, format_bad, format_bye, format_capability, format_exists, format_flags, format_list,
    format_no, format_ok, format_quota, format_quotaroot, format_recent, parse_command,
    parse_search_criteria, parse_sequence_set, sequence_set_to_uids, ImapCommand, TaggedCommand,
};
use mailrs_mailbox::{
    Mailbox, MailboxStore, FLAG_ANSWERED, FLAG_DELETED, FLAG_DRAFT, FLAG_FLAGGED, FLAG_RECENT,
    FLAG_SEEN,
};

use crate::domain_store::DomainStore;
use crate::imap_format::{
    build_bodystructure, extract_body_section, extract_header_fields, extract_header_section,
    extract_mime_part, format_addr_list, format_imap_flags, format_internal_date,
    parse_generic_body_sections, parse_header_fields_request, parse_imap_flags, quote_or_nil,
};
use crate::inbound::auth_guard::{AuthCheck, AuthGuard};
use crate::users::UserStore;

/// convert string responses to bytes
fn strs_to_bytes(strs: Vec<String>) -> Vec<Vec<u8>> {
    strs.into_iter().map(|s| s.into_bytes()).collect()
}

/// result of handling an IMAP command
pub enum HandleResult {
    Responses(Vec<Vec<u8>>),
    NeedLiteral { continuation: Vec<u8>, size: u32 },
    EnterIdle { continuation: Vec<u8>, tag: String },
}

/// pending APPEND state
struct PendingAppend {
    tag: String,
    mailbox: String,
    flags: u32,
}

/// IMAP session state machine
pub struct ImapSession {
    pub mailbox_store: Arc<MailboxStore>,
    users: Arc<UserStore>,
    state: ImapState,
    pending_append: Option<PendingAppend>,
    pub maildir_root: String,
    auth_guard: Option<Arc<AuthGuard>>,
    peer_addr: Option<std::net::IpAddr>,
    domain_store: Option<Arc<DomainStore>>,
    ldap_config: Option<Arc<crate::ldap_auth::LdapConfig>>,
}

enum ImapState {
    NotAuthenticated,
    Authenticated { username: String },
    Selected { username: String, mailbox: Mailbox },
}

impl ImapSession {
    pub fn new(mailbox_store: Arc<MailboxStore>, users: Arc<UserStore>) -> Self {
        Self {
            mailbox_store,
            users,
            state: ImapState::NotAuthenticated,
            pending_append: None,
            maildir_root: String::new(),
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

    pub fn with_domain_store(mut self, store: Arc<DomainStore>) -> Self {
        self.domain_store = Some(store);
        self
    }

    pub fn with_ldap_config(mut self, config: Arc<crate::ldap_auth::LdapConfig>) -> Self {
        self.ldap_config = Some(config);
        self
    }

    /// process a raw command line, return result
    pub async fn handle_line(&mut self, line: &str) -> HandleResult {
        // log all commands for debugging
        {
            let username = match &self.state {
                ImapState::Selected { username, .. } | ImapState::Authenticated { username } => {
                    username.as_str()
                }
                _ => "?",
            };
            // hide passwords in LOGIN commands
            let display = if line.to_uppercase().contains("LOGIN") {
                let parts: Vec<&str> = line.splitn(4, ' ').collect();
                if parts.len() >= 4 {
                    format!("{} {} {} ***", parts[0], parts[1], parts[2])
                } else {
                    line.trim().to_string()
                }
            } else {
                line.trim().to_string()
            };
            eprintln!("IMAP [{username}] << {display}");
        }
        match parse_command(line) {
            Ok(cmd) => self.handle_command(&cmd).await,
            Err(e) => {
                // try to extract tag for error response
                let tag = line.split_whitespace().next().unwrap_or("*");
                HandleResult::Responses(strs_to_bytes(vec![format_bad(
                    tag,
                    &format!("parse error: {e}"),
                )]))
            }
        }
    }

    async fn handle_command(&mut self, cmd: &TaggedCommand) -> HandleResult {
        let tag = &cmd.tag;

        // FETCH returns Vec<Vec<u8>> directly (binary-safe)
        if let ImapCommand::Fetch {
            sequence,
            attributes,
        } = &cmd.command
        {
            return HandleResult::Responses(
                self.handle_fetch(tag, sequence, attributes, false).await,
            );
        }

        // UID might contain FETCH
        if let ImapCommand::Uid { subcommand } = &cmd.command {
            return HandleResult::Responses(self.handle_uid(tag, subcommand.as_ref()).await);
        }

        // all other commands return Vec<String>, convert to Vec<Vec<u8>>
        let responses = match &cmd.command {
            ImapCommand::Capability => self.handle_capability(tag),
            ImapCommand::Login { username, password } => {
                self.handle_login(tag, username, password).await
            }
            ImapCommand::Logout => self.handle_logout(tag),
            ImapCommand::Noop => vec![format_ok(tag, "NOOP completed")],
            ImapCommand::List { reference, pattern } => {
                self.handle_list(tag, reference, pattern).await
            }
            ImapCommand::Select { mailbox } => self.handle_select(tag, mailbox).await,
            ImapCommand::Examine { mailbox } => self.handle_examine(tag, mailbox).await,
            ImapCommand::Store {
                sequence,
                action,
                flags,
            } => self.handle_store(tag, sequence, action, flags, false).await,
            ImapCommand::Search { criteria } => self.handle_search(tag, criteria).await,
            ImapCommand::Expunge => self.handle_expunge(tag).await,
            ImapCommand::Close => self.handle_close(tag).await,
            ImapCommand::Idle => {
                return self.handle_idle(tag);
            }
            ImapCommand::GetQuota { quotaroot } => self.handle_getquota(tag, quotaroot).await,
            ImapCommand::GetQuotaRoot { mailbox } => self.handle_getquotaroot(tag, mailbox).await,
            ImapCommand::Append {
                mailbox,
                flags,
                literal_size,
            } => {
                return self
                    .handle_append_start(tag, mailbox, flags.as_deref(), *literal_size)
                    .await;
            }
            ImapCommand::Copy { sequence, mailbox } => {
                self.handle_copy(tag, sequence, mailbox, false).await
            }
            ImapCommand::Move { sequence, mailbox } => {
                self.handle_move(tag, sequence, mailbox, false).await
            }
            ImapCommand::Status { mailbox, items } => {
                self.handle_status(tag, mailbox, items).await
            }
            ImapCommand::Create { mailbox } => self.handle_create(tag, mailbox).await,
            ImapCommand::Delete { mailbox } => self.handle_delete(tag, mailbox).await,
            ImapCommand::Rename { from, to } => self.handle_rename(tag, from, to).await,
            ImapCommand::Subscribe { mailbox: _ } => {
                vec![format_ok(tag, "SUBSCRIBE completed")]
            }
            ImapCommand::Unsubscribe { mailbox: _ } => {
                vec![format_ok(tag, "UNSUBSCRIBE completed")]
            }
            ImapCommand::Lsub { reference, pattern } => {
                self.handle_lsub(tag, reference, pattern).await
            }
            ImapCommand::Namespace => self.handle_namespace(tag),
            ImapCommand::Sort { criteria, search_criteria, .. } => {
                self.handle_sort(tag, criteria, search_criteria, false).await
            }
            ImapCommand::Enable(caps) => self.handle_enable(tag, caps),
            ImapCommand::Unselect => self.handle_unselect(tag),
            _ => unreachable!(), // Fetch and Uid handled above
        };
        HandleResult::Responses(strs_to_bytes(responses))
    }

    fn handle_capability(&self, tag: &str) -> Vec<String> {
        vec![
            format_capability(&["IMAP4rev1", "AUTH=PLAIN", "IDLE", "QUOTA", "CONDSTORE", "SPECIAL-USE", "NAMESPACE", "SORT", "ENABLE", "UNSELECT"]),
            format_ok(tag, "CAPABILITY completed"),
        ]
    }

    fn handle_namespace(&self, tag: &str) -> Vec<String> {
        vec![
            "* NAMESPACE ((\"\" \"/\")) NIL NIL\r\n".to_string(),
            format_ok(tag, "NAMESPACE completed"),
        ]
    }

    async fn handle_login(&mut self, tag: &str, username: &str, password: &str) -> Vec<String> {
        if matches!(
            self.state,
            ImapState::Authenticated { .. } | ImapState::Selected { .. }
        ) {
            return vec![format_bad(tag, "already authenticated")];
        }

        if let (Some(guard), Some(ip)) = (&self.auth_guard, self.peer_addr) {
            if let AuthCheck::LockedOut { remaining_secs } = guard.check(ip, username) {
                return vec![format_no(
                    tag,
                    &format!("Too many auth failures, try again in {remaining_secs}s"),
                )];
            }
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
                guard.record_success(ip, username);
            }
            let _ = self.mailbox_store.ensure_default_mailboxes(username).await;
            self.state = ImapState::Authenticated {
                username: username.to_string(),
            };
            vec![format_ok(tag, "LOGIN completed")]
        } else {
            if let (Some(guard), Some(ip)) = (&self.auth_guard, self.peer_addr) {
                guard.record_failure(ip, username);
            }
            vec![format_no(tag, "LOGIN failed")]
        }
    }

    fn handle_logout(&mut self, tag: &str) -> Vec<String> {
        self.state = ImapState::NotAuthenticated;
        vec![
            format_bye("server logging out"),
            format_ok(tag, "LOGOUT completed"),
        ]
    }

    async fn handle_list(&self, tag: &str, _reference: &str, pattern: &str) -> Vec<String> {
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

    async fn handle_create(&self, tag: &str, mailbox: &str) -> Vec<String> {
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

    async fn handle_delete(&self, tag: &str, mailbox: &str) -> Vec<String> {
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

    async fn handle_rename(&self, tag: &str, from: &str, to: &str) -> Vec<String> {
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

    async fn handle_select(&mut self, tag: &str, mailbox_name: &str) -> Vec<String> {
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

    async fn handle_examine(&mut self, tag: &str, mailbox_name: &str) -> Vec<String> {
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

    async fn handle_fetch(
        &self,
        tag: &str,
        sequence: &str,
        attributes: &str,
        use_uid: bool,
    ) -> Vec<Vec<u8>> {
        let mailbox = match &self.state {
            ImapState::Selected { mailbox, .. } => mailbox,
            _ => return strs_to_bytes(vec![format_no(tag, "no mailbox selected")]),
        };

        let seq_set = match parse_sequence_set(sequence) {
            Ok(s) => s,
            Err(e) => {
                return strs_to_bytes(vec![format_bad(tag, &format!("invalid sequence: {e}"))])
            }
        };

        // get message count for sequence expansion
        let (total, _) = self
            .mailbox_store
            .mailbox_status(mailbox.id)
            .await
            .unwrap_or((0, 0));

        let uids = if use_uid {
            // re-read uidnext from DB (cached value may be stale)
            let current_uidnext = self
                .mailbox_store
                .get_mailbox_by_id(mailbox.id)
                .await
                .ok()
                .flatten()
                .map(|m| m.uidnext)
                .unwrap_or(mailbox.uidnext);
            sequence_set_to_uids(&seq_set, current_uidnext.saturating_sub(1))
        } else {
            sequence_set_to_uids(&seq_set, total)
        };

        // parse requested attributes
        let attrs_upper = attributes.to_uppercase();
        let want_flags = attrs_upper.contains("FLAGS");
        let want_uid = attrs_upper.contains("UID") || use_uid;
        let want_rfc822_size = attrs_upper.contains("RFC822.SIZE");
        let want_internaldate = attrs_upper.contains("INTERNALDATE");
        let want_envelope = attrs_upper.contains("ENVELOPE");
        let want_body_peek = attrs_upper.contains("BODY.PEEK[]");
        // check for standalone RFC822 (not RFC822.SIZE, RFC822.HEADER, RFC822.TEXT)
        let has_standalone_rfc822 = attrs_upper
            .split_whitespace()
            .any(|w| w == "RFC822" || w == "(RFC822" || w == "RFC822)");
        let want_body_full =
            !want_body_peek && (attrs_upper.contains("BODY[]") || has_standalone_rfc822);
        let want_body_header = attrs_upper.contains("BODY[HEADER]")
            || attrs_upper.contains("BODY.PEEK[HEADER]")
            || attrs_upper.contains("RFC822.HEADER");
        let want_body_text = attrs_upper.contains("BODY[TEXT]")
            || attrs_upper.contains("BODY.PEEK[TEXT]")
            || attrs_upper.contains("RFC822.TEXT");
        let want_bodystructure = attrs_upper.contains("BODYSTRUCTURE");
        let want_modseq = attrs_upper.contains("MODSEQ");

        // BODY[HEADER.FIELDS (field-list)] / BODY.PEEK[HEADER.FIELDS (field-list)]
        let header_fields_request = parse_header_fields_request(attributes);

        // generic BODY[section] requests (e.g. BODY[1], BODY[1.1], BODY[1.MIME])
        let generic_body_sections = parse_generic_body_sections(attributes);

        let want_any_body = want_body_peek
            || want_body_full
            || want_body_header
            || want_body_text
            || header_fields_request.is_some()
            || !generic_body_sections.is_empty();

        // CHANGEDSINCE modifier (RFC 7162)
        let changedsince = if let Some(pos) = attrs_upper.find("CHANGEDSINCE") {
            let after = &attributes[pos + "CHANGEDSINCE".len()..];
            let after = after.trim_start();
            after.split_whitespace().next().and_then(|s| {
                // strip trailing parenthesis if present
                s.trim_end_matches(')').parse::<u64>().ok()
            })
        } else {
            None
        };

        let messages = match self
            .mailbox_store
            .list_messages(mailbox.id, 0, total.max(1000))
            .await
        {
            Ok(msgs) => msgs,
            Err(_) => return strs_to_bytes(vec![format_no(tag, "FETCH failed")]),
        };

        let mut responses = Vec::new();

        for msg in &messages {
            let seq_num = if use_uid {
                // check if this UID is in the requested set
                if !uids.contains(&msg.uid) {
                    continue;
                }
                // find sequence number (1-based position in the list)
                messages
                    .iter()
                    .position(|m| m.uid == msg.uid)
                    .map(|p| p as u32 + 1)
                    .unwrap_or(0)
            } else {
                let seq = messages
                    .iter()
                    .position(|m| m.uid == msg.uid)
                    .map(|p| p as u32 + 1)
                    .unwrap_or(0);
                if !uids.contains(&seq) {
                    continue;
                }
                seq
            };

            if seq_num == 0 {
                continue;
            }

            // CHANGEDSINCE filter: skip messages not modified since the given modseq
            if let Some(since) = changedsince {
                if msg.modseq <= since {
                    continue;
                }
            }

            // items are built as Vec<u8> to handle binary literal data correctly
            let mut items: Vec<Vec<u8>> = Vec::new();
            if want_flags {
                items.push(format!("FLAGS ({})", format_imap_flags(msg.flags)).into_bytes());
            }
            if want_uid {
                items.push(format!("UID {}", msg.uid).into_bytes());
            }
            if want_rfc822_size {
                items.push(format!("RFC822.SIZE {}", msg.size).into_bytes());
            }
            if want_internaldate {
                items.push(
                    format!(
                        "INTERNALDATE \"{}\"",
                        format_internal_date(msg.internal_date)
                    )
                    .into_bytes(),
                );
            }
            if want_envelope {
                let date = format_internal_date(msg.internal_date);
                let from = format_addr_list(&msg.sender);
                let to = format_addr_list(&msg.recipients);
                items.push(
                    format!(
                        "ENVELOPE ({} {} {} {} {} {} NIL NIL {} {})",
                        quote_or_nil(&date),
                        quote_or_nil(&msg.subject),
                        from,
                        from,
                        from,
                        to,
                        quote_or_nil(&msg.in_reply_to),
                        quote_or_nil(&msg.message_id),
                    )
                    .into_bytes(),
                );
            }

            if want_modseq || changedsince.is_some() {
                items.push(format!("MODSEQ ({})", msg.modseq).into_bytes());
            }

            if want_any_body || want_bodystructure {
                if let Some(data) = self.read_message_file(msg) {
                    if want_bodystructure {
                        items.push(
                            format!("BODYSTRUCTURE {}", build_bodystructure(&data)).into_bytes(),
                        );
                    }
                    // binary-safe literal builder: prefix + raw bytes
                    if want_body_header {
                        let header = extract_header_section(&data);
                        let mut item =
                            format!("BODY[HEADER] {{{}}}\r\n", header.len()).into_bytes();
                        item.extend_from_slice(&header);
                        items.push(item);
                    }
                    if want_body_text {
                        let body = extract_body_section(&data);
                        let mut item = format!("BODY[TEXT] {{{}}}\r\n", body.len()).into_bytes();
                        item.extend_from_slice(&body);
                        items.push(item);
                    }
                    if want_body_peek || want_body_full {
                        let mut item = format!("BODY[] {{{}}}\r\n", data.len()).into_bytes();
                        item.extend_from_slice(&data);
                        items.push(item);
                        if want_body_full {
                            let _ = self
                                .mailbox_store
                                .add_flags(mailbox.id, msg.uid, FLAG_SEEN)
                                .await;
                        }
                    }
                    if let Some((ref fields, ref raw_section)) = header_fields_request {
                        let filtered = extract_header_fields(&data, fields);
                        let mut item =
                            format!("BODY[{raw_section}] {{{}}}\r\n", filtered.len()).into_bytes();
                        item.extend_from_slice(&filtered);
                        items.push(item);
                    }
                    for section in &generic_body_sections {
                        let part_data = extract_mime_part(&data, section)
                            .unwrap_or_else(|| extract_body_section(&data));
                        let is_peek = attrs_upper.contains("PEEK");
                        let mut item =
                            format!("BODY[{section}] {{{}}}\r\n", part_data.len()).into_bytes();
                        item.extend_from_slice(&part_data);
                        items.push(item);
                        if !is_peek {
                            let _ = self
                                .mailbox_store
                                .add_flags(mailbox.id, msg.uid, FLAG_SEEN)
                                .await;
                        }
                    }
                }
            }

            // build the full FETCH response line as bytes
            let mut resp = format!("* {} FETCH (", seq_num).into_bytes();
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    resp.push(b' ');
                }
                resp.extend_from_slice(item);
            }
            resp.extend_from_slice(b")\r\n");
            responses.push(resp);
        }

        responses.push(format_ok(tag, "FETCH completed").into_bytes());
        responses
    }

    async fn handle_store(
        &self,
        tag: &str,
        sequence: &str,
        action: &str,
        flags_str: &str,
        use_uid: bool,
    ) -> Vec<String> {
        let mailbox = match &self.state {
            ImapState::Selected { mailbox, .. } => mailbox,
            _ => return vec![format_no(tag, "no mailbox selected")],
        };

        let seq_set = match parse_sequence_set(sequence) {
            Ok(s) => s,
            Err(e) => return vec![format_bad(tag, &format!("invalid sequence: {e}"))],
        };

        let (total, _) = self
            .mailbox_store
            .mailbox_status(mailbox.id)
            .await
            .unwrap_or((0, 0));

        let uids = if use_uid {
            let current_uidnext = self
                .mailbox_store
                .get_mailbox_by_id(mailbox.id)
                .await
                .ok()
                .flatten()
                .map(|m| m.uidnext)
                .unwrap_or(mailbox.uidnext);
            sequence_set_to_uids(&seq_set, current_uidnext.saturating_sub(1))
        } else {
            sequence_set_to_uids(&seq_set, total)
        };

        // parse UNCHANGEDSINCE modifier (RFC 7162)
        // format: STORE seq (UNCHANGEDSINCE modseq) +FLAGS (...)
        // parser splits: action = "(UNCHANGEDSINCE", flags = "modseq) +FLAGS (...)"
        let action_upper = action.to_uppercase();
        let (unchangedsince, real_action, real_flags) =
            if action_upper.starts_with("(UNCHANGEDSINCE") {
                // extract modseq from flags_str: "12345) +FLAGS (\Seen)"
                if let Some(paren_end) = flags_str.find(')') {
                    let modseq_str = flags_str[..paren_end].trim();
                    let rest = flags_str[paren_end + 1..].trim();
                    let modseq = modseq_str.parse::<u64>().ok();
                    // rest is "+FLAGS (\Seen)" — split into action and flags
                    if let Some((act, flg)) = rest.split_once(' ') {
                        (modseq, act.to_uppercase(), flg.to_string())
                    } else {
                        (modseq, rest.to_uppercase(), String::new())
                    }
                } else {
                    (None, action_upper.clone(), flags_str.to_string())
                }
            } else {
                (None, action_upper.clone(), flags_str.to_string())
            };

        let flag_bits = parse_imap_flags(&real_flags);

        let messages = match self
            .mailbox_store
            .list_messages(mailbox.id, 0, total.max(1000))
            .await
        {
            Ok(msgs) => msgs,
            Err(_) => return vec![format_no(tag, "STORE failed")],
        };

        let mut responses = Vec::new();
        let mut modified_uids: Vec<u32> = Vec::new();

        for msg in &messages {
            let (seq_num, target_uid) = if use_uid {
                if !uids.contains(&msg.uid) {
                    continue;
                }
                let seq = messages
                    .iter()
                    .position(|m| m.uid == msg.uid)
                    .map(|p| p as u32 + 1)
                    .unwrap_or(0);
                (seq, msg.uid)
            } else {
                let seq = messages
                    .iter()
                    .position(|m| m.uid == msg.uid)
                    .map(|p| p as u32 + 1)
                    .unwrap_or(0);
                if !uids.contains(&seq) {
                    continue;
                }
                (seq, msg.uid)
            };

            if seq_num == 0 {
                continue;
            }

            // UNCHANGEDSINCE: use conditional update
            if let Some(modseq_limit) = unchangedsince {
                let flag_action = if real_action.starts_with('+') {
                    mailrs_mailbox::FlagAction::Add
                } else if real_action.starts_with('-') {
                    mailrs_mailbox::FlagAction::Remove
                } else {
                    mailrs_mailbox::FlagAction::Set
                };

                match self
                    .mailbox_store
                    .update_flags_if_unchanged(
                        mailbox.id,
                        target_uid,
                        flag_bits,
                        flag_action,
                        modseq_limit,
                    )
                    .await
                {
                    Ok(Some(_modseq)) => {}
                    Ok(None) => {
                        // precondition failed — collect for MODIFIED response
                        modified_uids.push(target_uid);
                        continue;
                    }
                    Err(_) => return vec![format_no(tag, "STORE failed")],
                }
            } else {
                let result = if real_action.starts_with('+') {
                    self.mailbox_store
                        .add_flags(mailbox.id, target_uid, flag_bits)
                        .await
                } else if real_action.starts_with('-') {
                    self.mailbox_store
                        .remove_flags(mailbox.id, target_uid, flag_bits)
                        .await
                } else {
                    self.mailbox_store
                        .update_flags(mailbox.id, target_uid, flag_bits)
                        .await
                };

                if result.is_err() {
                    return vec![format_no(tag, "STORE failed")];
                }
            }

            // fetch updated flags + modseq
            if let Ok(Some(updated)) = self.mailbox_store.get_message(mailbox.id, target_uid).await
            {
                if !real_action.contains(".SILENT") {
                    if unchangedsince.is_some() {
                        responses.push(format!(
                            "* {} FETCH (FLAGS ({}) MODSEQ ({}))\r\n",
                            seq_num,
                            format_imap_flags(updated.flags),
                            updated.modseq,
                        ));
                    } else {
                        responses.push(format!(
                            "* {} FETCH (FLAGS ({}))\r\n",
                            seq_num,
                            format_imap_flags(updated.flags)
                        ));
                    }
                }
            }
        }

        if !modified_uids.is_empty() {
            let uid_list: Vec<String> = modified_uids.iter().map(|u| u.to_string()).collect();
            responses.push(format_ok(
                tag,
                &format!("[MODIFIED {}] STORE completed", uid_list.join(",")),
            ));
        } else {
            responses.push(format_ok(tag, "STORE completed"));
        }
        responses
    }

    async fn handle_search(&self, tag: &str, criteria: &str) -> Vec<String> {
        let mailbox = match &self.state {
            ImapState::Selected { mailbox, .. } => mailbox,
            _ => return vec![format_no(tag, "no mailbox selected")],
        };

        let (total, _) = self
            .mailbox_store
            .mailbox_status(mailbox.id)
            .await
            .unwrap_or((0, 0));

        let messages = match self
            .mailbox_store
            .list_messages(mailbox.id, 0, total.max(1000))
            .await
        {
            Ok(msgs) => msgs,
            Err(_) => return vec![format_no(tag, "SEARCH failed")],
        };

        let keys = parse_search_criteria(criteria);
        let mut matching_seqs: Vec<u32> = Vec::new();

        for (i, msg) in messages.iter().enumerate() {
            let seq = i as u32 + 1;
            if message_matches_criteria(msg, &keys) {
                matching_seqs.push(seq);
            }
        }

        let seq_list: Vec<String> = matching_seqs.iter().map(|s| s.to_string()).collect();
        vec![
            format!("* SEARCH {}\r\n", seq_list.join(" ")),
            format_ok(tag, "SEARCH completed"),
        ]
    }

    async fn handle_sort(&self, tag: &str, criteria: &str, search_criteria: &str, uid_mode: bool) -> Vec<String> {
        let mailbox = match &self.state {
            ImapState::Selected { mailbox, .. } => mailbox,
            _ => return vec![format_no(tag, "no mailbox selected")],
        };

        let (total, _) = self
            .mailbox_store
            .mailbox_status(mailbox.id)
            .await
            .unwrap_or((0, 0));

        let messages = match self
            .mailbox_store
            .list_messages(mailbox.id, 0, total.max(1000))
            .await
        {
            Ok(msgs) => msgs,
            Err(_) => return vec![format_no(tag, "SORT failed")],
        };

        // filter by search criteria
        let keys = parse_search_criteria(search_criteria);
        let mut filtered: Vec<(usize, &mailrs_mailbox::MessageMeta)> = messages
            .iter()
            .enumerate()
            .filter(|(_, msg)| message_matches_criteria(msg, &keys))
            .collect();

        // sort by criteria
        let sort_keys = parse_sort_criteria(criteria);
        filtered.sort_by(|a, b| {
            for key in &sort_keys {
                let ord = match key {
                    SortCriterion::Arrival => a.1.internal_date.cmp(&b.1.internal_date),
                    SortCriterion::Date => a.1.date.cmp(&b.1.date),
                    SortCriterion::From => a.1.sender.to_lowercase().cmp(&b.1.sender.to_lowercase()),
                    SortCriterion::Subject => a.1.subject.to_lowercase().cmp(&b.1.subject.to_lowercase()),
                    SortCriterion::Size => a.1.size.cmp(&b.1.size),
                    SortCriterion::ReverseArrival => b.1.internal_date.cmp(&a.1.internal_date),
                    SortCriterion::ReverseDate => b.1.date.cmp(&a.1.date),
                    SortCriterion::ReverseFrom => b.1.sender.to_lowercase().cmp(&a.1.sender.to_lowercase()),
                    SortCriterion::ReverseSubject => b.1.subject.to_lowercase().cmp(&a.1.subject.to_lowercase()),
                    SortCriterion::ReverseSize => b.1.size.cmp(&a.1.size),
                };
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            std::cmp::Ordering::Equal
        });

        let ids: Vec<String> = if uid_mode {
            filtered.iter().map(|(_, msg)| msg.uid.to_string()).collect()
        } else {
            filtered.iter().map(|(i, _)| (i + 1).to_string()).collect()
        };

        vec![
            format!("* SORT {}\r\n", ids.join(" ")),
            format_ok(tag, "SORT completed"),
        ]
    }

    async fn handle_expunge(&self, tag: &str) -> Vec<String> {
        let mailbox = match &self.state {
            ImapState::Selected { mailbox, .. } => mailbox,
            _ => return vec![format_no(tag, "no mailbox selected")],
        };

        let expunged = match self.mailbox_store.expunge(mailbox.id).await {
            Ok(uids) => uids,
            Err(_) => return vec![format_no(tag, "EXPUNGE failed")],
        };

        let mut responses: Vec<String> = expunged
            .iter()
            .map(|uid| format!("* {uid} EXPUNGE\r\n"))
            .collect();

        responses.push(format_ok(tag, "EXPUNGE completed"));
        responses
    }

    async fn handle_close(&mut self, tag: &str) -> Vec<String> {
        // expunge deleted messages and return to authenticated state
        if let ImapState::Selected { mailbox, username } = &self.state {
            let _ = self.mailbox_store.expunge(mailbox.id).await;
            self.state = ImapState::Authenticated {
                username: username.clone(),
            };
        }
        vec![format_ok(tag, "CLOSE completed")]
    }

    fn handle_unselect(&mut self, tag: &str) -> Vec<String> {
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

    fn handle_enable(&self, tag: &str, capabilities: &[String]) -> Vec<String> {
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

    async fn handle_getquota(&self, tag: &str, quotaroot: &str) -> Vec<String> {
        let username = match &self.state {
            ImapState::Authenticated { username } | ImapState::Selected { username, .. } => {
                username
            }
            ImapState::NotAuthenticated => return vec![format_no(tag, "not authenticated")],
        };

        // quotaroot is the username (user-level quota)
        if quotaroot != username {
            return vec![format_no(tag, "permission denied")];
        }

        let usage = self.mailbox_store.user_storage_usage(username).await;
        let quota = if let Some(ref ds) = self.domain_store {
            ds.get_quota(username)
                .await
                .ok()
                .flatten()
                .unwrap_or(1_073_741_824)
        } else {
            1_073_741_824 // default 1GB
        };

        // IMAP QUOTA uses KB
        let usage_kb = usage / 1024;
        let limit_kb = quota as u64 / 1024;

        vec![
            format_quota(quotaroot, usage_kb, limit_kb),
            format_ok(tag, "GETQUOTA completed"),
        ]
    }

    async fn handle_getquotaroot(&self, tag: &str, mailbox: &str) -> Vec<String> {
        let username = match &self.state {
            ImapState::Authenticated { username } | ImapState::Selected { username, .. } => {
                username
            }
            ImapState::NotAuthenticated => return vec![format_no(tag, "not authenticated")],
        };

        let usage = self.mailbox_store.user_storage_usage(username).await;
        let quota = if let Some(ref ds) = self.domain_store {
            ds.get_quota(username)
                .await
                .ok()
                .flatten()
                .unwrap_or(1_073_741_824)
        } else {
            1_073_741_824
        };

        let usage_kb = usage / 1024;
        let limit_kb = quota as u64 / 1024;

        vec![
            format_quotaroot(mailbox, username),
            format_quota(username, usage_kb, limit_kb),
            format_ok(tag, "GETQUOTAROOT completed"),
        ]
    }

    async fn handle_status(&self, tag: &str, mailbox: &str, items: &str) -> Vec<String> {
        let username = match &self.state {
            ImapState::Authenticated { username } | ImapState::Selected { username, .. } => {
                username
            }
            ImapState::NotAuthenticated => return vec![format_no(tag, "not authenticated")],
        };

        if !mailbox.eq_ignore_ascii_case("INBOX") {
            return vec![format!("* STATUS \"{}\" (MESSAGES 0 RECENT 0 UNSEEN 0)", mailbox),
                        format_ok(tag, "STATUS completed")];
        }

        let total = self.mailbox_store.count_messages(username).await;
        let unseen = self.mailbox_store.count_unseen(username).await;

        let mut parts = Vec::new();
        let items_upper = items.to_uppercase();
        if items_upper.contains("MESSAGES") {
            parts.push(format!("MESSAGES {total}"));
        }
        if items_upper.contains("RECENT") {
            parts.push("RECENT 0".to_string());
        }
        if items_upper.contains("UIDNEXT") {
            parts.push(format!("UIDNEXT {}", total + 1));
        }
        if items_upper.contains("UIDVALIDITY") {
            parts.push("UIDVALIDITY 1".to_string());
        }
        if items_upper.contains("UNSEEN") {
            parts.push(format!("UNSEEN {unseen}"));
        }

        vec![
            format!("* STATUS \"INBOX\" ({})", parts.join(" ")),
            format_ok(tag, "STATUS completed"),
        ]
    }

    async fn handle_lsub(&self, tag: &str, _reference: &str, _pattern: &str) -> Vec<String> {
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

    fn handle_idle(&self, tag: &str) -> HandleResult {
        match &self.state {
            ImapState::Selected { .. } | ImapState::Authenticated { .. } => {
                HandleResult::EnterIdle {
                    continuation: b"+ idling\r\n".to_vec(),
                    tag: tag.to_string(),
                }
            }
            ImapState::NotAuthenticated => {
                HandleResult::Responses(strs_to_bytes(vec![format_no(tag, "not authenticated")]))
            }
        }
    }

    /// return current user if authenticated
    pub fn idle_user(&self) -> Option<&str> {
        match &self.state {
            ImapState::Authenticated { username } | ImapState::Selected { username, .. } => {
                Some(username.as_str())
            }
            ImapState::NotAuthenticated => None,
        }
    }

    /// return selected mailbox id if in Selected state
    pub fn selected_mailbox_id(&self) -> Option<i64> {
        match &self.state {
            ImapState::Selected { mailbox, .. } => Some(mailbox.id),
            _ => None,
        }
    }

    /// generate status update responses for the selected mailbox
    pub async fn idle_status_update(&self) -> Vec<Vec<u8>> {
        if let Some(mb_id) = self.selected_mailbox_id() {
            if let Ok((total, _)) = self.mailbox_store.mailbox_status(mb_id).await {
                return strs_to_bytes(vec![format_exists(total)]);
            }
        }
        Vec::new()
    }

    async fn handle_copy(
        &self,
        tag: &str,
        sequence: &str,
        dest_mailbox: &str,
        use_uid: bool,
    ) -> Vec<String> {
        let (username, mailbox) = match &self.state {
            ImapState::Selected { username, mailbox } => (username.clone(), mailbox),
            _ => return vec![format_no(tag, "no mailbox selected")],
        };

        let seq_set = match parse_sequence_set(sequence) {
            Ok(s) => s,
            Err(e) => return vec![format_bad(tag, &format!("invalid sequence: {e}"))],
        };

        let (total, _) = self
            .mailbox_store
            .mailbox_status(mailbox.id)
            .await
            .unwrap_or((0, 0));
        let uids = if use_uid {
            let current_uidnext = self
                .mailbox_store
                .get_mailbox_by_id(mailbox.id)
                .await
                .ok()
                .flatten()
                .map(|m| m.uidnext)
                .unwrap_or(mailbox.uidnext);
            sequence_set_to_uids(&seq_set, current_uidnext.saturating_sub(1))
        } else {
            sequence_set_to_uids(&seq_set, total)
        };

        let messages = match self
            .mailbox_store
            .list_messages(mailbox.id, 0, total.max(1000))
            .await
        {
            Ok(msgs) => msgs,
            Err(_) => return vec![format_no(tag, "COPY failed")],
        };

        for msg in &messages {
            let matches = if use_uid {
                uids.contains(&msg.uid)
            } else {
                let seq = messages
                    .iter()
                    .position(|m| m.uid == msg.uid)
                    .map(|p| p as u32 + 1)
                    .unwrap_or(0);
                uids.contains(&seq)
            };
            if matches
                && self
                    .mailbox_store
                    .copy_message(&username, mailbox.id, msg.uid, dest_mailbox)
                    .await
                    .is_err()
            {
                return vec![format_no(tag, "COPY failed")];
            }
        }

        vec![format_ok(tag, "COPY completed")]
    }

    async fn handle_move(
        &self,
        tag: &str,
        sequence: &str,
        dest_mailbox: &str,
        use_uid: bool,
    ) -> Vec<String> {
        let (username, mailbox) = match &self.state {
            ImapState::Selected { username, mailbox } => (username.clone(), mailbox),
            _ => return vec![format_no(tag, "no mailbox selected")],
        };

        let seq_set = match parse_sequence_set(sequence) {
            Ok(s) => s,
            Err(e) => return vec![format_bad(tag, &format!("invalid sequence: {e}"))],
        };

        let (total, _) = self
            .mailbox_store
            .mailbox_status(mailbox.id)
            .await
            .unwrap_or((0, 0));
        let uids = if use_uid {
            let current_uidnext = self
                .mailbox_store
                .get_mailbox_by_id(mailbox.id)
                .await
                .ok()
                .flatten()
                .map(|m| m.uidnext)
                .unwrap_or(mailbox.uidnext);
            sequence_set_to_uids(&seq_set, current_uidnext.saturating_sub(1))
        } else {
            sequence_set_to_uids(&seq_set, total)
        };

        let messages = match self
            .mailbox_store
            .list_messages(mailbox.id, 0, total.max(1000))
            .await
        {
            Ok(msgs) => msgs,
            Err(_) => return vec![format_no(tag, "MOVE failed")],
        };

        let mut expunged = Vec::new();
        for (i, msg) in messages.iter().enumerate() {
            let seq = i as u32 + 1;
            let matches = if use_uid {
                uids.contains(&msg.uid)
            } else {
                uids.contains(&seq)
            };
            if matches {
                if self
                    .mailbox_store
                    .move_message(&username, mailbox.id, msg.uid, dest_mailbox)
                    .await
                    .is_err()
                {
                    return vec![format_no(tag, "MOVE failed")];
                }
                expunged.push(seq);
            }
        }

        let mut responses: Vec<String> = expunged
            .iter()
            .map(|s| format!("* {s} EXPUNGE\r\n"))
            .collect();
        responses.push(format_ok(tag, "MOVE completed"));
        responses
    }

    /// read raw message bytes from maildir
    fn read_message_file(&self, msg: &mailrs_mailbox::MessageMeta) -> Option<Vec<u8>> {
        let username = match &self.state {
            ImapState::Selected { username, .. } => username,
            _ => return None,
        };
        let (local, domain) = username.split_once('@')?;
        let base = format!("{}/{domain}/{local}", self.maildir_root);

        // fast path: try direct file lookup by maildir_id
        // check new/ (no flags suffix)
        let new_path = format!("{base}/new/{}", msg.maildir_id);
        if let Ok(data) = std::fs::read(&new_path) {
            return Some(data);
        }
        // check cur/ with common flag suffixes
        for suffix in &[":2,S", ":2,", ":2,RS", ":2,FS", ":2,FRS"] {
            let cur_path = format!("{base}/cur/{}{suffix}", msg.maildir_id);
            if let Ok(data) = std::fs::read(&cur_path) {
                return Some(data);
            }
        }

        // slow fallback: scan directories
        let md = mailrs_storage_maildir::Maildir::open(&base);
        let find_in = |entries: Vec<mailrs_storage_maildir::Entry>| -> Option<Vec<u8>> {
            entries
                .into_iter()
                .find(|e| e.id.to_string() == msg.maildir_id)
                .and_then(|e| std::fs::read(&e.path).ok())
        };
        find_in(md.scan_cur().unwrap_or_default())
            .or_else(|| find_in(md.scan_new().unwrap_or_default()))
    }

    async fn handle_uid(&mut self, tag: &str, subcommand: &ImapCommand) -> Vec<Vec<u8>> {
        match subcommand {
            ImapCommand::Fetch {
                sequence,
                attributes,
            } => self.handle_fetch(tag, sequence, attributes, true).await,
            ImapCommand::Store {
                sequence,
                action,
                flags,
            } => strs_to_bytes(self.handle_store(tag, sequence, action, flags, true).await),
            ImapCommand::Search { criteria } => {
                // UID SEARCH returns UIDs instead of sequence numbers
                let mailbox = match &self.state {
                    ImapState::Selected { mailbox, .. } => mailbox,
                    _ => return strs_to_bytes(vec![format_no(tag, "no mailbox selected")]),
                };

                let (total, _) = self
                    .mailbox_store
                    .mailbox_status(mailbox.id)
                    .await
                    .unwrap_or((0, 0));

                let messages = match self
                    .mailbox_store
                    .list_messages(mailbox.id, 0, total.max(1000))
                    .await
                {
                    Ok(msgs) => msgs,
                    Err(_) => return strs_to_bytes(vec![format_no(tag, "SEARCH failed")]),
                };

                let keys = parse_search_criteria(criteria);
                let mut matching_uids: Vec<u32> = Vec::new();

                for msg in &messages {
                    if message_matches_criteria(msg, &keys) {
                        matching_uids.push(msg.uid);
                    }
                }

                let uid_list: Vec<String> = matching_uids.iter().map(|u| u.to_string()).collect();
                strs_to_bytes(vec![
                    format!("* SEARCH {}\r\n", uid_list.join(" ")),
                    format_ok(tag, "UID SEARCH completed"),
                ])
            }
            ImapCommand::Copy { sequence, mailbox } => {
                strs_to_bytes(self.handle_copy(tag, sequence, mailbox, true).await)
            }
            ImapCommand::Move { sequence, mailbox } => {
                strs_to_bytes(self.handle_move(tag, sequence, mailbox, true).await)
            }
            ImapCommand::Sort { criteria, search_criteria, .. } => {
                strs_to_bytes(self.handle_sort(tag, criteria, search_criteria, true).await)
            }
            _ => strs_to_bytes(vec![format_bad(tag, "unsupported UID subcommand")]),
        }
    }

    async fn handle_append_start(
        &mut self,
        tag: &str,
        mailbox: &str,
        flags: Option<&str>,
        literal_size: u32,
    ) -> HandleResult {
        let username = match &self.state {
            ImapState::Authenticated { username } | ImapState::Selected { username, .. } => {
                username.clone()
            }
            ImapState::NotAuthenticated => {
                return HandleResult::Responses(strs_to_bytes(vec![format_no(
                    tag,
                    "not authenticated",
                )]));
            }
        };

        // verify mailbox exists
        match self.mailbox_store.get_mailbox(&username, mailbox).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return HandleResult::Responses(strs_to_bytes(vec![format_no(
                    tag,
                    "[TRYCREATE] mailbox not found",
                )]));
            }
            Err(_) => {
                return HandleResult::Responses(strs_to_bytes(vec![format_no(
                    tag,
                    "APPEND failed",
                )]));
            }
        }

        let flag_bits = flags.map(parse_imap_flags).unwrap_or(0);

        self.pending_append = Some(PendingAppend {
            tag: tag.to_string(),
            mailbox: mailbox.to_string(),
            flags: flag_bits,
        });

        HandleResult::NeedLiteral {
            continuation: b"+ Ready for literal data\r\n".to_vec(),
            size: literal_size,
        }
    }

    /// handle literal data (for APPEND)
    pub async fn handle_literal_data(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        let pending = match self.pending_append.take() {
            Some(p) => p,
            None => return strs_to_bytes(vec!["* BAD unexpected literal data\r\n".to_string()]),
        };

        let username = match &self.state {
            ImapState::Authenticated { username } | ImapState::Selected { username, .. } => {
                username.clone()
            }
            ImapState::NotAuthenticated => {
                return strs_to_bytes(vec![format_no(&pending.tag, "not authenticated")]);
            }
        };

        let now = chrono::Utc::now().timestamp();
        match self
            .mailbox_store
            .append_message(
                &username,
                &pending.mailbox,
                &self.maildir_root,
                data,
                pending.flags,
                now,
            )
            .await
        {
            Ok((uid, _)) => strs_to_bytes(vec![format_ok(
                &pending.tag,
                &format!(
                    "[APPENDUID {} {uid}] APPEND completed",
                    self.mailbox_store
                        .get_mailbox(&username, &pending.mailbox)
                        .await
                        .ok()
                        .flatten()
                        .map(|m| m.uidvalidity)
                        .unwrap_or(0)
                ),
            )]),
            Err(e) => strs_to_bytes(vec![format_no(
                &pending.tag,
                &format!("APPEND failed: {e}"),
            )]),
        }
    }
}

/// check if a message matches all search criteria (AND semantics)
/// sort criterion parsed from SORT command
enum SortCriterion {
    Arrival,
    Date,
    From,
    Subject,
    Size,
    ReverseArrival,
    ReverseDate,
    ReverseFrom,
    ReverseSubject,
    ReverseSize,
}

/// parse sort criteria string like "REVERSE DATE" or "ARRIVAL"
fn parse_sort_criteria(criteria: &str) -> Vec<SortCriterion> {
    let tokens: Vec<&str> = criteria.split_whitespace().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i].to_uppercase();
        if token == "REVERSE" && i + 1 < tokens.len() {
            let next = tokens[i + 1].to_uppercase();
            match next.as_str() {
                "ARRIVAL" => result.push(SortCriterion::ReverseArrival),
                "DATE" => result.push(SortCriterion::ReverseDate),
                "FROM" => result.push(SortCriterion::ReverseFrom),
                "SUBJECT" => result.push(SortCriterion::ReverseSubject),
                "SIZE" | "RFC822.SIZE" => result.push(SortCriterion::ReverseSize),
                _ => {}
            }
            i += 2;
        } else {
            match token.as_str() {
                "ARRIVAL" => result.push(SortCriterion::Arrival),
                "DATE" => result.push(SortCriterion::Date),
                "FROM" => result.push(SortCriterion::From),
                "SUBJECT" => result.push(SortCriterion::Subject),
                "SIZE" | "RFC822.SIZE" => result.push(SortCriterion::Size),
                _ => {}
            }
            i += 1;
        }
    }
    result
}

fn message_matches_criteria(msg: &mailrs_mailbox::MessageMeta, keys: &[SearchKey]) -> bool {
    // seconds per day for date comparisons
    const DAY: i64 = 86400;

    for key in keys {
        let matches = match key {
            SearchKey::All => true,
            SearchKey::Seen => msg.flags & FLAG_SEEN != 0,
            SearchKey::Unseen => msg.flags & FLAG_SEEN == 0,
            SearchKey::Flagged => msg.flags & FLAG_FLAGGED != 0,
            SearchKey::Unflagged => msg.flags & FLAG_FLAGGED == 0,
            SearchKey::Answered => msg.flags & FLAG_ANSWERED != 0,
            SearchKey::Unanswered => msg.flags & FLAG_ANSWERED == 0,
            SearchKey::Deleted => msg.flags & FLAG_DELETED != 0,
            SearchKey::Undeleted => msg.flags & FLAG_DELETED == 0,
            SearchKey::Draft => msg.flags & FLAG_DRAFT != 0,
            SearchKey::Undraft => msg.flags & FLAG_DRAFT == 0,
            SearchKey::Recent => msg.flags & FLAG_RECENT != 0,
            SearchKey::From(pattern) => {
                msg.sender.to_lowercase().contains(&pattern.to_lowercase())
            }
            SearchKey::To(pattern) => {
                msg.recipients
                    .to_lowercase()
                    .contains(&pattern.to_lowercase())
            }
            SearchKey::Subject(pattern) => {
                msg.subject
                    .to_lowercase()
                    .contains(&pattern.to_lowercase())
            }
            SearchKey::Text(pattern) => {
                let p = pattern.to_lowercase();
                msg.subject.to_lowercase().contains(&p)
                    || msg.sender.to_lowercase().contains(&p)
                    || msg.recipients.to_lowercase().contains(&p)
            }
            SearchKey::Body(pattern) => {
                // body search requires reading message content, which is expensive
                // fall back to subject search as a best-effort approximation
                msg.subject
                    .to_lowercase()
                    .contains(&pattern.to_lowercase())
            }
            SearchKey::Since(ts) => msg.date >= *ts,
            SearchKey::Before(ts) => msg.date < *ts,
            SearchKey::On(ts) => {
                let day_start = *ts;
                let day_end = day_start + DAY;
                msg.date >= day_start && msg.date < day_end
            }
            SearchKey::Uid(seq_str) => match parse_sequence_set(seq_str) {
                Ok(set) => {
                    let uids = sequence_set_to_uids(&set, u32::MAX);
                    uids.contains(&msg.uid)
                }
                Err(_) => false,
            },
        };
        if !matches {
            return false;
        }
    }
    true
}

/// format IMAP connection greeting
pub fn imap_greeting(hostname: &str) -> Vec<u8> {
    format!("* OK [{hostname}] IMAP4rev1 server ready\r\n").into_bytes()
}


#[cfg(test)]
mod tests {
    use super::*;
    // additional imports not already brought in by super::* from module-level use
    use crate::imap_format::{
        escape_imap_str, escape_imap_string, find_line_offset, format_imap_address,
        split_mime_parts, trim_part_trailing_newline,
    };

    /// requires MAILRS_PG_URL env var pointing to a test database
    async fn test_session() -> ImapSession {
        let url =
            std::env::var("MAILRS_PG_URL").expect("MAILRS_PG_URL required for integration tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        let store = Arc::new(MailboxStore::new(pool));
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
}
