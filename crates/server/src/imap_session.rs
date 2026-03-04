use std::sync::Arc;

use mailrs_imap_proto::{
    ImapCommand, TaggedCommand, parse_command,
    format_bad, format_bye, format_capability, format_exists, format_flags, format_list,
    format_no, format_ok, format_quota, format_quotaroot, format_recent,
    parse_sequence_set, sequence_set_to_uids,
};
use mailrs_mailbox::{
    MailboxStore, Mailbox,
    FLAG_SEEN, FLAG_ANSWERED, FLAG_FLAGGED, FLAG_DELETED, FLAG_DRAFT, FLAG_RECENT,
};

use crate::domain_store::DomainStore;
use crate::inbound::auth_guard::{AuthCheck, AuthGuard};
use crate::users::UserStore;

/// result of handling an IMAP command
pub enum HandleResult {
    Responses(Vec<String>),
    NeedLiteral { continuation: String, size: u32 },
    EnterIdle { continuation: String, tag: String },
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

    /// process a raw command line, return result
    pub async fn handle_line(&mut self, line: &str) -> HandleResult {
        // log all commands for debugging
        {
            let username = match &self.state {
                ImapState::Selected { username, .. } | ImapState::Authenticated { username } => username.as_str(),
                _ => "?",
            };
            // hide passwords in LOGIN commands
            let display = if line.to_uppercase().contains("LOGIN") {
                let parts: Vec<&str> = line.splitn(4, ' ').collect();
                if parts.len() >= 4 { format!("{} {} {} ***", parts[0], parts[1], parts[2]) } else { line.trim().to_string() }
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
                HandleResult::Responses(vec![format_bad(tag, &format!("parse error: {e}"))])
            }
        }
    }

    async fn handle_command(&mut self, cmd: &TaggedCommand) -> HandleResult {
        let tag = &cmd.tag;
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
            ImapCommand::Fetch { sequence, attributes } => {
                self.handle_fetch(tag, sequence, attributes, false).await
            }
            ImapCommand::Store { sequence, action, flags } => {
                self.handle_store(tag, sequence, action, flags, false).await
            }
            ImapCommand::Search { criteria } => self.handle_search(tag, criteria).await,
            ImapCommand::Expunge => self.handle_expunge(tag).await,
            ImapCommand::Close => self.handle_close(tag).await,
            ImapCommand::Idle => {
                return self.handle_idle(tag);
            }
            ImapCommand::GetQuota { quotaroot } => self.handle_getquota(tag, quotaroot).await,
            ImapCommand::GetQuotaRoot { mailbox } => self.handle_getquotaroot(tag, mailbox).await,
            ImapCommand::Append { mailbox, flags, literal_size } => {
                return self.handle_append_start(tag, mailbox, flags.as_deref(), *literal_size).await;
            }
            ImapCommand::Copy { sequence, mailbox } => {
                self.handle_copy(tag, sequence, mailbox, false).await
            }
            ImapCommand::Move { sequence, mailbox } => {
                self.handle_move(tag, sequence, mailbox, false).await
            }
            ImapCommand::Uid { subcommand } => self.handle_uid(tag, subcommand.as_ref()).await,
        };
        HandleResult::Responses(responses)
    }

    fn handle_capability(&self, tag: &str) -> Vec<String> {
        vec![
            format_capability(&["IMAP4rev1", "AUTH=PLAIN", "IDLE", "QUOTA", "CONDSTORE"]),
            format_ok(tag, "CAPABILITY completed"),
        ]
    }

    async fn handle_login(&mut self, tag: &str, username: &str, password: &str) -> Vec<String> {
        if matches!(self.state, ImapState::Authenticated { .. } | ImapState::Selected { .. }) {
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

        // try users.toml first, then PG accounts table
        let ok = if self.users.verify(username, password) {
            true
        } else if let Some(ref ds) = self.domain_store {
            match ds.get_account_with_hash(username).await {
                Ok(Some((_account, hash))) => {
                    if hash.starts_with("$argon2") {
                        crate::users::UserStore::verify_hash(password, &hash)
                    } else {
                        hash == password
                    }
                }
                _ => false,
            }
        } else {
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
                let flags = "\\HasNoChildren";
                responses.push(format_list(flags, "/", &mb.name));
            }
        }
        responses.push(format_ok(tag, "LIST completed"));
        responses
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

        match self.mailbox_store.get_mailbox(&username, mailbox_name).await {
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
                    format!("* OK [HIGHESTMODSEQ {}] highest modseq\r\n", mb.highest_modseq),
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

        match self.mailbox_store.get_mailbox(&username, mailbox_name).await {
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
                    format!("* OK [HIGHESTMODSEQ {}] highest modseq\r\n", mb.highest_modseq),
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
    ) -> Vec<String> {
        let mailbox = match &self.state {
            ImapState::Selected { mailbox, .. } => mailbox,
            _ => return vec![format_no(tag, "no mailbox selected")],
        };

        let seq_set = match parse_sequence_set(sequence) {
            Ok(s) => s,
            Err(e) => return vec![format_bad(tag, &format!("invalid sequence: {e}"))],
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
        let has_standalone_rfc822 = attrs_upper.split_whitespace()
            .any(|w| w == "RFC822" || w == "(RFC822" || w == "RFC822)");
        let want_body_full = !want_body_peek && (attrs_upper.contains("BODY[]") || has_standalone_rfc822);
        let want_body_header = attrs_upper.contains("BODY[HEADER]") || attrs_upper.contains("BODY.PEEK[HEADER]") || attrs_upper.contains("RFC822.HEADER");
        let want_body_text = attrs_upper.contains("BODY[TEXT]") || attrs_upper.contains("BODY.PEEK[TEXT]") || attrs_upper.contains("RFC822.TEXT");
        let want_bodystructure = attrs_upper.contains("BODYSTRUCTURE");
        let want_modseq = attrs_upper.contains("MODSEQ");

        // BODY[HEADER.FIELDS (field-list)] / BODY.PEEK[HEADER.FIELDS (field-list)]
        let header_fields_request = parse_header_fields_request(attributes);

        // generic BODY[section] requests (e.g. BODY[1], BODY[1.1], BODY[1.MIME])
        let generic_body_sections = parse_generic_body_sections(attributes);

        let want_any_body = want_body_peek || want_body_full || want_body_header || want_body_text
            || header_fields_request.is_some() || !generic_body_sections.is_empty();

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

        let messages = match self.mailbox_store.list_messages(mailbox.id, 0, total.max(1000)).await {
            Ok(msgs) => msgs,
            Err(_) => return vec![format_no(tag, "FETCH failed")],
        };

        let mut responses = Vec::new();

        for msg in &messages {
            let seq_num = if use_uid {
                // check if this UID is in the requested set
                if !uids.contains(&msg.uid) {
                    continue;
                }
                // find sequence number (1-based position in the list)
                messages.iter().position(|m| m.uid == msg.uid).map(|p| p as u32 + 1).unwrap_or(0)
            } else {
                let seq = messages.iter().position(|m| m.uid == msg.uid).map(|p| p as u32 + 1).unwrap_or(0);
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

            let mut items = Vec::new();
            if want_flags {
                items.push(format!("FLAGS ({})", format_imap_flags(msg.flags)));
            }
            if want_uid {
                items.push(format!("UID {}", msg.uid));
            }
            if want_rfc822_size {
                items.push(format!("RFC822.SIZE {}", msg.size));
            }
            if want_internaldate {
                items.push(format!(
                    "INTERNALDATE \"{}\"",
                    format_internal_date(msg.internal_date)
                ));
            }
            if want_envelope {
                // RFC 3501: ENVELOPE (date subject from sender reply-to to cc bcc in-reply-to message-id)
                let date = format_internal_date(msg.internal_date);
                let from = format_addr_list(&msg.sender);
                let to = format_addr_list(&msg.recipients);
                items.push(format!(
                    "ENVELOPE ({} {} {} {} {} {} NIL NIL {} {})",
                    quote_or_nil(&date),
                    quote_or_nil(&msg.subject),
                    from,          // from
                    from,          // sender = from
                    from,          // reply-to = from
                    to,            // to
                    quote_or_nil(&msg.in_reply_to),
                    quote_or_nil(&msg.message_id),
                ));
            }

            if want_modseq || changedsince.is_some() {
                items.push(format!("MODSEQ ({})", msg.modseq));
            }

            if want_any_body || want_bodystructure {
                if let Some(data) = self.read_message_file(msg) {
                    // BODYSTRUCTURE first (non-literal, before any literals)
                    if want_bodystructure {
                        items.push(format!("BODYSTRUCTURE {}", build_bodystructure(&data)));
                    }
                    // then literal items
                    if want_body_header {
                        let header = extract_header_section(&data);
                        items.push(format!("BODY[HEADER] {{{}}}\r\n{}", header.len(), String::from_utf8_lossy(&header)));
                    }
                    if want_body_text {
                        let body = extract_body_section(&data);
                        items.push(format!("BODY[TEXT] {{{}}}\r\n{}", body.len(), String::from_utf8_lossy(&body)));
                    }
                    if want_body_peek || want_body_full {
                        items.push(format!("BODY[] {{{}}}\r\n{}", data.len(), String::from_utf8_lossy(&data)));
                        if want_body_full {
                            let _ = self.mailbox_store.add_flags(mailbox.id, msg.uid, FLAG_SEEN).await;
                        }
                    }
                    if let Some((ref fields, ref raw_section)) = header_fields_request {
                        let filtered = extract_header_fields(&data, fields);
                        items.push(format!("BODY[{raw_section}] {{{}}}\r\n{}", filtered.len(), String::from_utf8_lossy(&filtered)));
                    }
                    for section in &generic_body_sections {
                        let part_data = extract_mime_part(&data, section)
                            .unwrap_or_else(|| extract_body_section(&data));
                        let is_peek = attrs_upper.contains("PEEK");
                        items.push(format!("BODY[{section}] {{{}}}\r\n{}", part_data.len(), String::from_utf8_lossy(&part_data)));
                        if !is_peek {
                            let _ = self.mailbox_store.add_flags(mailbox.id, msg.uid, FLAG_SEEN).await;
                        }
                    }
                }
            }

            responses.push(format!(
                "* {} FETCH ({})\r\n",
                seq_num,
                items.join(" ")
            ));
        }

        responses.push(format_ok(tag, "FETCH completed"));
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
        let (unchangedsince, real_action, real_flags) = if action_upper.starts_with("(UNCHANGEDSINCE") {
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

        let messages = match self.mailbox_store.list_messages(mailbox.id, 0, total.max(1000)).await {
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
                let seq = messages.iter().position(|m| m.uid == msg.uid).map(|p| p as u32 + 1).unwrap_or(0);
                (seq, msg.uid)
            } else {
                let seq = messages.iter().position(|m| m.uid == msg.uid).map(|p| p as u32 + 1).unwrap_or(0);
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

                match self.mailbox_store.update_flags_if_unchanged(
                    mailbox.id, target_uid, flag_bits, flag_action, modseq_limit,
                ).await {
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
                    self.mailbox_store.add_flags(mailbox.id, target_uid, flag_bits).await
                } else if real_action.starts_with('-') {
                    self.mailbox_store.remove_flags(mailbox.id, target_uid, flag_bits).await
                } else {
                    self.mailbox_store.update_flags(mailbox.id, target_uid, flag_bits).await
                };

                if result.is_err() {
                    return vec![format_no(tag, "STORE failed")];
                }
            }

            // fetch updated flags + modseq
            if let Ok(Some(updated)) = self.mailbox_store.get_message(mailbox.id, target_uid).await {
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

        let messages = match self.mailbox_store.list_messages(mailbox.id, 0, total.max(1000)).await {
            Ok(msgs) => msgs,
            Err(_) => return vec![format_no(tag, "SEARCH failed")],
        };

        let criteria_upper = criteria.to_uppercase();
        let mut matching_seqs: Vec<u32> = Vec::new();

        for (i, msg) in messages.iter().enumerate() {
            let seq = i as u32 + 1;
            let matches = if criteria_upper.contains("UNSEEN") {
                msg.flags & FLAG_SEEN == 0
            } else if criteria_upper.contains("SEEN") {
                msg.flags & FLAG_SEEN != 0
            } else if criteria_upper.contains("FLAGGED") {
                msg.flags & FLAG_FLAGGED != 0
            } else if criteria_upper.contains("DELETED") {
                msg.flags & FLAG_DELETED != 0
            } else {
                // ALL, empty, or unknown criteria: return all
                true
            };

            if matches {
                matching_seqs.push(seq);
            }
        }

        let seq_list: Vec<String> = matching_seqs.iter().map(|s| s.to_string()).collect();
        vec![
            format!("* SEARCH {}\r\n", seq_list.join(" ")),
            format_ok(tag, "SEARCH completed"),
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

    async fn handle_getquota(&self, tag: &str, quotaroot: &str) -> Vec<String> {
        let username = match &self.state {
            ImapState::Authenticated { username } | ImapState::Selected { username, .. } => username,
            ImapState::NotAuthenticated => return vec![format_no(tag, "not authenticated")],
        };

        // quotaroot is the username (user-level quota)
        if quotaroot != username {
            return vec![format_no(tag, "permission denied")];
        }

        let usage = self.mailbox_store.user_storage_usage(username).await;
        let quota = if let Some(ref ds) = self.domain_store {
            ds.get_quota(username).await.ok().flatten().unwrap_or(1_073_741_824)
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
            ImapState::Authenticated { username } | ImapState::Selected { username, .. } => username,
            ImapState::NotAuthenticated => return vec![format_no(tag, "not authenticated")],
        };

        let usage = self.mailbox_store.user_storage_usage(username).await;
        let quota = if let Some(ref ds) = self.domain_store {
            ds.get_quota(username).await.ok().flatten().unwrap_or(1_073_741_824)
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

    fn handle_idle(&self, tag: &str) -> HandleResult {
        match &self.state {
            ImapState::Selected { .. } => HandleResult::EnterIdle {
                continuation: "+ idling\r\n".to_string(),
                tag: tag.to_string(),
            },
            ImapState::Authenticated { .. } => HandleResult::EnterIdle {
                continuation: "+ idling\r\n".to_string(),
                tag: tag.to_string(),
            },
            ImapState::NotAuthenticated => {
                HandleResult::Responses(vec![format_no(tag, "not authenticated")])
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
    pub async fn idle_status_update(&self) -> Vec<String> {
        if let Some(mb_id) = self.selected_mailbox_id() {
            if let Ok((total, _)) = self.mailbox_store.mailbox_status(mb_id).await {
                return vec![format_exists(total)];
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

        let (total, _) = self.mailbox_store.mailbox_status(mailbox.id).await.unwrap_or((0, 0));
        let uids = if use_uid {
            let current_uidnext = self.mailbox_store.get_mailbox_by_id(mailbox.id).await.ok().flatten().map(|m| m.uidnext).unwrap_or(mailbox.uidnext);
            sequence_set_to_uids(&seq_set, current_uidnext.saturating_sub(1))
        } else {
            sequence_set_to_uids(&seq_set, total)
        };

        let messages = match self.mailbox_store.list_messages(mailbox.id, 0, total.max(1000)).await {
            Ok(msgs) => msgs,
            Err(_) => return vec![format_no(tag, "COPY failed")],
        };

        for msg in &messages {
            let matches = if use_uid { uids.contains(&msg.uid) } else {
                let seq = messages.iter().position(|m| m.uid == msg.uid).map(|p| p as u32 + 1).unwrap_or(0);
                uids.contains(&seq)
            };
            if matches {
                if let Err(_) = self.mailbox_store.copy_message(&username, mailbox.id, msg.uid, dest_mailbox).await {
                    return vec![format_no(tag, "COPY failed")];
                }
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

        let (total, _) = self.mailbox_store.mailbox_status(mailbox.id).await.unwrap_or((0, 0));
        let uids = if use_uid {
            let current_uidnext = self.mailbox_store.get_mailbox_by_id(mailbox.id).await.ok().flatten().map(|m| m.uidnext).unwrap_or(mailbox.uidnext);
            sequence_set_to_uids(&seq_set, current_uidnext.saturating_sub(1))
        } else {
            sequence_set_to_uids(&seq_set, total)
        };

        let messages = match self.mailbox_store.list_messages(mailbox.id, 0, total.max(1000)).await {
            Ok(msgs) => msgs,
            Err(_) => return vec![format_no(tag, "MOVE failed")],
        };

        let mut expunged = Vec::new();
        for (i, msg) in messages.iter().enumerate() {
            let seq = i as u32 + 1;
            let matches = if use_uid { uids.contains(&msg.uid) } else { uids.contains(&seq) };
            if matches {
                if let Err(_) = self.mailbox_store.move_message(&username, mailbox.id, msg.uid, dest_mailbox).await {
                    return vec![format_no(tag, "MOVE failed")];
                }
                expunged.push(seq);
            }
        }

        let mut responses: Vec<String> = expunged.iter().map(|s| format!("* {s} EXPUNGE\r\n")).collect();
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

    async fn handle_uid(&mut self, tag: &str, subcommand: &ImapCommand) -> Vec<String> {
        match subcommand {
            ImapCommand::Fetch { sequence, attributes } => {
                self.handle_fetch(tag, sequence, attributes, true).await
            }
            ImapCommand::Store { sequence, action, flags } => {
                self.handle_store(tag, sequence, action, flags, true).await
            }
            ImapCommand::Search { criteria } => {
                // UID SEARCH returns UIDs instead of sequence numbers
                let mailbox = match &self.state {
                    ImapState::Selected { mailbox, .. } => mailbox,
                    _ => return vec![format_no(tag, "no mailbox selected")],
                };

                let (total, _) = self
                    .mailbox_store
                    .mailbox_status(mailbox.id)
                    .await
                    .unwrap_or((0, 0));

                let messages = match self.mailbox_store.list_messages(mailbox.id, 0, total.max(1000)).await {
                    Ok(msgs) => msgs,
                    Err(_) => return vec![format_no(tag, "SEARCH failed")],
                };

                let criteria_upper = criteria.to_uppercase();
                let mut matching_uids: Vec<u32> = Vec::new();

                for msg in &messages {
                    let matches = if criteria_upper.contains("UNSEEN") {
                        msg.flags & FLAG_SEEN == 0
                    } else if criteria_upper.contains("SEEN") {
                        msg.flags & FLAG_SEEN != 0
                    } else if criteria_upper.contains("FLAGGED") {
                        msg.flags & FLAG_FLAGGED != 0
                    } else if criteria_upper.contains("DELETED") {
                        msg.flags & FLAG_DELETED != 0
                    } else {
                        true
                    };

                    if matches {
                        matching_uids.push(msg.uid);
                    }
                }

                let uid_list: Vec<String> = matching_uids.iter().map(|u| u.to_string()).collect();
                vec![
                    format!("* SEARCH {}\r\n", uid_list.join(" ")),
                    format_ok(tag, "UID SEARCH completed"),
                ]
            }
            ImapCommand::Copy { sequence, mailbox } => {
                self.handle_copy(tag, sequence, mailbox, true).await
            }
            ImapCommand::Move { sequence, mailbox } => {
                self.handle_move(tag, sequence, mailbox, true).await
            }
            _ => vec![format_bad(tag, "unsupported UID subcommand")],
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
                return HandleResult::Responses(vec![format_no(tag, "not authenticated")]);
            }
        };

        // verify mailbox exists
        match self.mailbox_store.get_mailbox(&username, mailbox).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return HandleResult::Responses(vec![format_no(
                    tag,
                    "[TRYCREATE] mailbox not found",
                )]);
            }
            Err(_) => {
                return HandleResult::Responses(vec![format_no(tag, "APPEND failed")]);
            }
        }

        let flag_bits = flags.map(|f| parse_imap_flags(f)).unwrap_or(0);

        self.pending_append = Some(PendingAppend {
            tag: tag.to_string(),
            mailbox: mailbox.to_string(),
            flags: flag_bits,
        });

        HandleResult::NeedLiteral {
            continuation: "+ Ready for literal data\r\n".to_string(),
            size: literal_size,
        }
    }

    /// handle literal data (for APPEND)
    pub async fn handle_literal_data(&mut self, data: &[u8]) -> Vec<String> {
        let pending = match self.pending_append.take() {
            Some(p) => p,
            None => return vec!["* BAD unexpected literal data\r\n".to_string()],
        };

        let username = match &self.state {
            ImapState::Authenticated { username } | ImapState::Selected { username, .. } => {
                username.clone()
            }
            ImapState::NotAuthenticated => {
                return vec![format_no(&pending.tag, "not authenticated")];
            }
        };

        let now = chrono::Utc::now().timestamp();
        match self.mailbox_store.append_message(
            &username,
            &pending.mailbox,
            &self.maildir_root,
            data,
            pending.flags,
            now,
        ).await {
            Ok((uid, _)) => {
                vec![format_ok(
                    &pending.tag,
                    &format!("[APPENDUID {} {uid}] APPEND completed",
                        self.mailbox_store
                            .get_mailbox(&username, &pending.mailbox)
                            .await
                            .ok()
                            .flatten()
                            .map(|m| m.uidvalidity)
                            .unwrap_or(0)
                    ),
                )]
            }
            Err(e) => vec![format_no(&pending.tag, &format!("APPEND failed: {e}"))],
        }
    }
}

/// format IMAP connection greeting
pub fn imap_greeting(hostname: &str) -> String {
    format!("* OK [{hostname}] IMAP4rev1 server ready\r\n")
}

/// convert bitmask flags to IMAP flag string
fn format_imap_flags(flags: u32) -> String {
    let mut parts = Vec::new();
    if flags & FLAG_SEEN != 0 {
        parts.push("\\Seen");
    }
    if flags & FLAG_ANSWERED != 0 {
        parts.push("\\Answered");
    }
    if flags & FLAG_FLAGGED != 0 {
        parts.push("\\Flagged");
    }
    if flags & FLAG_DELETED != 0 {
        parts.push("\\Deleted");
    }
    if flags & FLAG_DRAFT != 0 {
        parts.push("\\Draft");
    }
    if flags & FLAG_RECENT != 0 {
        parts.push("\\Recent");
    }
    parts.join(" ")
}

/// parse IMAP flag names from a FLAGS string like "(\\Seen \\Flagged)"
fn parse_imap_flags(s: &str) -> u32 {
    let s = s.trim().trim_start_matches('(').trim_end_matches(')');
    let mut bits = 0u32;
    for part in s.split_whitespace() {
        let flag = part.trim_start_matches('\\');
        match flag.to_uppercase().as_str() {
            "SEEN" => bits |= FLAG_SEEN,
            "ANSWERED" => bits |= FLAG_ANSWERED,
            "FLAGGED" => bits |= FLAG_FLAGGED,
            "DELETED" => bits |= FLAG_DELETED,
            "DRAFT" => bits |= FLAG_DRAFT,
            "RECENT" => bits |= FLAG_RECENT,
            _ => {}
        }
    }
    bits
}

fn format_internal_date(timestamp: i64) -> String {
    use chrono::DateTime;
    let dt = DateTime::from_timestamp(timestamp, 0).unwrap_or_default();
    dt.format("%d-%b-%Y %H:%M:%S %z").to_string()
}

fn escape_imap_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// quote a string for IMAP or return NIL if empty
fn quote_or_nil(s: &str) -> String {
    if s.is_empty() {
        "NIL".to_string()
    } else {
        format!("\"{}\"", escape_imap_string(s))
    }
}

/// parse an email address "Name <user@host>" or "user@host" into IMAP address structure
/// returns ((name NIL mailbox host)) or NIL if empty
fn format_imap_address(addr: &str) -> String {
    let addr = addr.trim();
    if addr.is_empty() {
        return "NIL".to_string();
    }

    // parse "Name <user@host>" format
    if let Some(lt) = addr.find('<') {
        let name = addr[..lt].trim().trim_matches('"');
        let email = addr[lt + 1..].trim_end_matches('>');
        let (mailbox, host) = email.split_once('@').unwrap_or((email, ""));
        let name_part = if name.is_empty() { "NIL".to_string() } else { format!("\"{}\"", escape_imap_string(name)) };
        return format!("(({name_part} NIL \"{}\" \"{}\"))", escape_imap_string(mailbox), escape_imap_string(host));
    }

    // plain "user@host"
    if let Some((mailbox, host)) = addr.split_once('@') {
        format!("((NIL NIL \"{}\" \"{}\"))", escape_imap_string(mailbox), escape_imap_string(host))
    } else {
        format!("((NIL NIL \"{}\" \"\"))", escape_imap_string(addr))
    }
}

/// parse BODY[HEADER.FIELDS (field-list)] or BODY.PEEK[HEADER.FIELDS (field-list)]
/// returns (field_names, raw_section_text) e.g. (["FROM","TO","SUBJECT"], "HEADER.FIELDS (FROM TO SUBJECT)")
fn parse_header_fields_request(attributes: &str) -> Option<(Vec<String>, String)> {
    let upper = attributes.to_uppercase();
    let marker = "HEADER.FIELDS";
    let pos = upper.find(marker)?;
    // find the opening paren after HEADER.FIELDS
    let after = &attributes[pos + marker.len()..];
    let paren_start = after.find('(')?;
    let paren_end = after.find(')')?;
    let fields_str = &after[paren_start + 1..paren_end];
    let fields: Vec<String> = fields_str
        .split_whitespace()
        .map(|s| s.to_uppercase())
        .collect();
    let raw_section = format!("HEADER.FIELDS ({})", fields_str.trim());
    Some((fields, raw_section))
}

/// parse all generic BODY[section] requests like BODY[1], BODY[1.1], BODY[1.MIME], BODY.PEEK[1]
/// returns all section specifiers (e.g. ["1", "1.1", "1.MIME"])
fn parse_generic_body_sections(attributes: &str) -> Vec<String> {
    let upper = attributes.to_uppercase();
    let mut sections = Vec::new();

    for prefix in &["BODY.PEEK[", "BODY["] {
        let mut search_from = 0;
        while let Some(rel_pos) = upper[search_from..].find(prefix) {
            let abs_start = search_from + rel_pos + prefix.len();
            if let Some(end_rel) = upper[abs_start..].find(']') {
                let section = attributes[abs_start..abs_start + end_rel].trim();
                // skip empty, HEADER, TEXT, HEADER.FIELDS — handled by other code paths
                let sec_upper = section.to_uppercase();
                if !section.is_empty()
                    && sec_upper != "HEADER"
                    && sec_upper != "TEXT"
                    && !sec_upper.contains("HEADER.FIELDS")
                    && section.as_bytes().first().map_or(false, |b| b.is_ascii_digit())
                {
                    let s = section.to_string();
                    if !sections.contains(&s) {
                        sections.push(s);
                    }
                }
                search_from = abs_start + end_rel + 1;
            } else {
                break;
            }
        }
    }

    sections
}

/// extract only the specified header fields from raw message
fn extract_header_fields(data: &[u8], fields: &[String]) -> Vec<u8> {
    let header = extract_header_section(data);
    let header_str = String::from_utf8_lossy(&header);
    let mut result = Vec::new();
    let mut lines = header_str.lines().peekable();
    let mut include = false;
    while let Some(line) = lines.next() {
        if line.is_empty() {
            break;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            // continuation line — include if previous header was included
            if include {
                result.extend_from_slice(line.as_bytes());
                result.extend_from_slice(b"\r\n");
            }
        } else {
            // new header line
            include = false;
            if let Some(colon) = line.find(':') {
                let name = line[..colon].trim().to_uppercase();
                if fields.contains(&name) {
                    include = true;
                    result.extend_from_slice(line.as_bytes());
                    result.extend_from_slice(b"\r\n");
                }
            }
        }
    }
    result.extend_from_slice(b"\r\n");
    result
}

/// extract header section from raw message (up to \r\n\r\n)
fn extract_header_section(data: &[u8]) -> Vec<u8> {
    if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
        data[..pos + 4].to_vec()
    } else if let Some(pos) = data.windows(2).position(|w| w == b"\n\n") {
        data[..pos + 2].to_vec()
    } else {
        data.to_vec()
    }
}

/// extract body section from raw message (after \r\n\r\n)
fn extract_body_section(data: &[u8]) -> Vec<u8> {
    if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
        data[pos + 4..].to_vec()
    } else if let Some(pos) = data.windows(2).position(|w| w == b"\n\n") {
        data[pos + 2..].to_vec()
    } else {
        Vec::new()
    }
}

/// parsed MIME content-type info
struct MimeInfo {
    media_type: String,
    subtype: String,
    charset: String,
    encoding: String,
    boundary: Option<String>,
}

/// parse content-type and transfer-encoding from header text
fn parse_mime_headers(header: &str) -> MimeInfo {
    let mut media_type = "TEXT".to_string();
    let mut subtype = "PLAIN".to_string();
    let mut charset = "UTF-8".to_string();
    let mut encoding = "7BIT".to_string();
    let mut boundary = None;

    // unfold headers (join continuation lines)
    let mut unfolded = String::new();
    for line in header.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            unfolded.push(' ');
            unfolded.push_str(line.trim());
        } else {
            if !unfolded.is_empty() {
                unfolded.push('\n');
            }
            unfolded.push_str(line);
        }
    }

    for line in unfolded.lines() {
        let lower = line.to_lowercase();
        if lower.starts_with("content-type:") {
            let val = line["content-type:".len()..].trim();
            let val_lower = val.to_lowercase();
            if val_lower.starts_with("text/html") || val_lower.contains("text/html") {
                media_type = "TEXT".to_string();
                subtype = "HTML".to_string();
            } else if val_lower.starts_with("text/plain") || val_lower.contains("text/plain") {
                media_type = "TEXT".to_string();
                subtype = "PLAIN".to_string();
            } else if val_lower.contains("multipart/") {
                media_type = "MULTIPART".to_string();
                if val_lower.contains("multipart/mixed") {
                    subtype = "MIXED".to_string();
                } else if val_lower.contains("multipart/alternative") {
                    subtype = "ALTERNATIVE".to_string();
                } else if val_lower.contains("multipart/related") {
                    subtype = "RELATED".to_string();
                } else {
                    subtype = "MIXED".to_string();
                }
            } else if val_lower.contains("application/") {
                media_type = "APPLICATION".to_string();
                if let Some(s) = val_lower.split('/').nth(1) {
                    subtype = s.split(';').next().unwrap_or("OCTET-STREAM").trim().to_uppercase();
                }
            } else if val_lower.contains("image/") {
                media_type = "IMAGE".to_string();
                if let Some(s) = val_lower.split('/').nth(1) {
                    subtype = s.split(';').next().unwrap_or("JPEG").trim().to_uppercase();
                }
            }
            // extract charset
            if let Some(pos) = val_lower.find("charset=") {
                let rest = &val[pos + 8..];
                let cs = rest.trim_start_matches('"')
                    .split(|c: char| c == '"' || c == ';' || c.is_whitespace())
                    .next().unwrap_or("UTF-8");
                charset = cs.to_uppercase();
            }
            // extract boundary
            if let Some(pos) = val_lower.find("boundary=") {
                let rest = &val[pos + 9..];
                let b = if rest.starts_with('"') {
                    rest[1..].split('"').next().unwrap_or("")
                } else {
                    rest.split(|c: char| c == ';' || c.is_whitespace()).next().unwrap_or("")
                };
                boundary = Some(b.to_string());
            }
        }
        if lower.starts_with("content-transfer-encoding:") {
            let val = line["content-transfer-encoding:".len()..].trim();
            encoding = match val.to_uppercase().as_str() {
                "BASE64" => "BASE64".to_string(),
                "QUOTED-PRINTABLE" => "QUOTED-PRINTABLE".to_string(),
                "8BIT" => "8BIT".to_string(),
                _ => "7BIT".to_string(),
            };
        }
    }

    MimeInfo { media_type, subtype, charset, encoding, boundary }
}

/// split multipart body by boundary, returning each part as raw bytes (including part headers)
fn split_mime_parts<'a>(body: &'a [u8], boundary: &str) -> Vec<&'a [u8]> {
    let delim = format!("--{boundary}");
    let body_str = String::from_utf8_lossy(body);
    let mut parts = Vec::new();

    let mut in_parts = false;
    let mut part_start = 0;

    for (i, line) in body_str.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with(&delim) {
            if trimmed == format!("--{boundary}--") || trimmed.starts_with(&format!("--{boundary}--")) {
                // closing boundary
                if in_parts {
                    // find byte offset of this line
                    if let Some(pos) = find_line_offset(body, &body_str, i) {
                        if pos > part_start {
                            parts.push(&body[part_start..pos]);
                        }
                    }
                }
                break;
            }
            if in_parts {
                // end of previous part
                if let Some(pos) = find_line_offset(body, &body_str, i) {
                    if pos > part_start {
                        parts.push(&body[part_start..pos]);
                    }
                }
            }
            // start of next part (after the boundary line + CRLF/LF)
            if let Some(pos) = find_line_offset(body, &body_str, i) {
                let after = pos + line.len();
                // skip CRLF or LF after boundary
                part_start = if body.get(after) == Some(&b'\r') && body.get(after + 1) == Some(&b'\n') {
                    after + 2
                } else if body.get(after) == Some(&b'\n') {
                    after + 1
                } else {
                    after
                };
            }
            in_parts = true;
        }
    }

    parts
}

/// find byte offset of line number in body (helper for split_mime_parts)
fn find_line_offset(body: &[u8], _body_str: &str, target_line: usize) -> Option<usize> {
    let mut line_num = 0;
    let mut pos = 0;
    while pos < body.len() {
        if line_num == target_line {
            return Some(pos);
        }
        // find next newline
        if let Some(nl) = body[pos..].iter().position(|&b| b == b'\n') {
            pos = pos + nl + 1;
        } else {
            pos = body.len();
        }
        line_num += 1;
    }
    if line_num == target_line { Some(pos) } else { None }
}

/// trim trailing CRLF/LF from part body (boundary transport padding per RFC 2046)
fn trim_part_trailing_newline(data: &[u8]) -> &[u8] {
    let mut end = data.len();
    if end >= 2 && data[end - 2] == b'\r' && data[end - 1] == b'\n' {
        end -= 2;
    } else if end >= 1 && data[end - 1] == b'\n' {
        end -= 1;
    }
    &data[..end]
}

/// build a single part's BODYSTRUCTURE string (with extension data)
fn build_part_bodystructure(part_data: &[u8]) -> String {
    let header = extract_header_section(part_data);
    let header_str = String::from_utf8_lossy(&header);
    let body = extract_body_section(part_data);
    let info = parse_mime_headers(&header_str);

    if info.media_type == "MULTIPART" {
        if let Some(ref boundary) = info.boundary {
            let parts = split_mime_parts(&body, boundary);
            // RFC 3501: 1*body SP media-subtype — parts concatenated directly, no space
            let parts_str: String = parts.iter()
                .map(|p| build_part_bodystructure(p))
                .collect();
            return format!(
                "({} \"{}\" (\"boundary\" \"{}\") NIL NIL)",
                parts_str, info.subtype.to_lowercase(), boundary,
            );
        }
        let body_trimmed = trim_part_trailing_newline(&body);
        let body_lines = body_trimmed.split(|&b| b == b'\n').count();
        return format!(
            "(\"text\" \"plain\" (\"charset\" \"UTF-8\") NIL NIL \"7bit\" {} {} NIL NIL NIL)",
            body_trimmed.len(), body_lines,
        );
    }

    // trim trailing CRLF — belongs to boundary delimiter, not content
    let body_trimmed = trim_part_trailing_newline(&body);

    if info.media_type == "TEXT" {
        let body_lines = body_trimmed.split(|&b| b == b'\n').count();
        format!(
            "(\"text\" \"{}\" (\"charset\" \"{}\") NIL NIL \"{}\" {} {} NIL NIL NIL)",
            info.subtype.to_lowercase(), info.charset,
            info.encoding.to_lowercase(), body_trimmed.len(), body_lines,
        )
    } else {
        format!(
            "(\"{}\" \"{}\" NIL NIL NIL \"{}\" {} NIL NIL NIL)",
            info.media_type.to_lowercase(), info.subtype.to_lowercase(),
            info.encoding.to_lowercase(), body_trimmed.len(),
        )
    }
}

/// build BODYSTRUCTURE for a message (handles multipart, with extension data)
fn build_bodystructure(data: &[u8]) -> String {
    let header_bytes = extract_header_section(data);
    let header = String::from_utf8_lossy(&header_bytes);
    let body = extract_body_section(data);
    let info = parse_mime_headers(&header);

    if info.media_type == "MULTIPART" {
        if let Some(ref boundary) = info.boundary {
            let parts = split_mime_parts(&body, boundary);
            if !parts.is_empty() {
                // RFC 3501: 1*body SP media-subtype — parts concatenated directly
                let parts_str: String = parts.iter()
                    .map(|p| build_part_bodystructure(p))
                    .collect();
                return format!(
                    "({} \"{}\" (\"boundary\" \"{}\") NIL NIL)",
                    parts_str, info.subtype.to_lowercase(), boundary,
                );
            }
        }
    }

    // single-part message
    if info.media_type == "TEXT" {
        let body_lines = body.split(|&b| b == b'\n').count();
        format!(
            "(\"text\" \"{}\" (\"charset\" \"{}\") NIL NIL \"{}\" {} {} NIL NIL NIL)",
            info.subtype.to_lowercase(), info.charset,
            info.encoding.to_lowercase(), body.len(), body_lines,
        )
    } else {
        format!(
            "(\"{}\" \"{}\" NIL NIL NIL \"{}\" {} NIL NIL NIL)",
            info.media_type.to_lowercase(), info.subtype.to_lowercase(),
            info.encoding.to_lowercase(), body.len(),
        )
    }
}

/// extract a specific MIME part by number (e.g. "1", "2", "1.1", "1.MIME")
fn extract_mime_part(data: &[u8], section: &str) -> Option<Vec<u8>> {
    // handle .MIME suffix — return MIME headers of the part
    let upper = section.to_uppercase();
    if upper.ends_with(".MIME") {
        let base = &section[..section.len() - 5];
        let part_raw = find_mime_part_raw(data, base)?;
        return Some(extract_header_section(&part_raw));
    }

    find_mime_part_body(data, section)
}

/// find a MIME part's raw data (headers + body) by section number
fn find_mime_part_raw(data: &[u8], section: &str) -> Option<Vec<u8>> {
    let header_bytes = extract_header_section(data);
    let header = String::from_utf8_lossy(&header_bytes);
    let body = extract_body_section(data);
    let info = parse_mime_headers(&header);

    if info.media_type != "MULTIPART" || info.boundary.is_none() {
        if section == "1" {
            return Some(data.to_vec());
        }
        return None;
    }

    let boundary = info.boundary.as_ref()?;
    let parts = split_mime_parts(&body, boundary);

    let mut parts_iter = section.split('.');
    let first: usize = parts_iter.next()?.parse().ok()?;
    let rest: String = parts_iter.collect::<Vec<_>>().join(".");

    if first == 0 || first > parts.len() {
        return None;
    }

    let part = parts[first - 1];
    if rest.is_empty() {
        Some(part.to_vec())
    } else {
        find_mime_part_raw(part, &rest)
    }
}

/// extract a specific MIME part's body by section number (e.g. "1", "2", "1.1")
fn find_mime_part_body(data: &[u8], section: &str) -> Option<Vec<u8>> {
    let header_bytes = extract_header_section(data);
    let header = String::from_utf8_lossy(&header_bytes);
    let body = extract_body_section(data);
    let info = parse_mime_headers(&header);

    if info.media_type != "MULTIPART" || info.boundary.is_none() {
        // single-part: section "1" means the body itself
        if section == "1" {
            return Some(body);
        }
        return None;
    }

    let boundary = info.boundary.as_ref()?;
    let parts = split_mime_parts(&body, boundary);

    // parse section like "1" or "1.2"
    let mut parts_iter = section.split('.');
    let first: usize = parts_iter.next()?.parse().ok()?;
    let rest: String = parts_iter.collect::<Vec<_>>().join(".");

    if first == 0 || first > parts.len() {
        return None;
    }

    let part = parts[first - 1];
    if rest.is_empty() {
        // return body of this part (after its headers)
        Some(extract_body_section(part))
    } else {
        // recurse into nested multipart
        find_mime_part_body(part, &rest)
    }
}

/// format a comma-separated list of addresses into IMAP address list
fn format_addr_list(addrs: &str) -> String {
    let addrs = addrs.trim();
    if addrs.is_empty() {
        return "NIL".to_string();
    }
    let parts: Vec<String> = addrs
        .split(',')
        .map(|a| {
            let a = a.trim();
            if a.is_empty() {
                return String::new();
            }
            // extract the inner tuple from ((...)
            let formatted = format_imap_address(a);
            if formatted == "NIL" {
                return String::new();
            }
            // strip outer parens to get inner (name NIL mailbox host)
            formatted[1..formatted.len() - 1].to_string()
        })
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        "NIL".to_string()
    } else {
        format!("({})", parts.join(""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// requires MAILRS_PG_URL env var pointing to a test database
    async fn test_session() -> ImapSession {
        let url = std::env::var("MAILRS_PG_URL")
            .expect("MAILRS_PG_URL required for integration tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        let store = Arc::new(MailboxStore::new(pool));
        let users = Arc::new(UserStore::from_plain_passwords(vec![
            ("alice@example.com".into(), "password123".into()),
        ]));
        ImapSession::new(store, users)
    }

    /// extract responses from HandleResult, panicking on NeedLiteral/EnterIdle
    fn responses(result: HandleResult) -> Vec<String> {
        match result {
            HandleResult::Responses(r) => r,
            HandleResult::NeedLiteral { .. } => panic!("unexpected NeedLiteral"),
            HandleResult::EnterIdle { .. } => panic!("unexpected EnterIdle"),
        }
    }

    #[tokio::test]
    #[ignore]
    async fn login_success() {
        let mut session = test_session().await;
        let resp = responses(session.handle_line("a001 LOGIN alice@example.com password123").await);
        assert!(resp.last().unwrap().contains("OK"));
        assert!(resp.last().unwrap().contains("LOGIN completed"));
    }

    #[tokio::test]
    #[ignore]
    async fn login_wrong_password() {
        let mut session = test_session().await;
        let resp = responses(session.handle_line("a001 LOGIN alice@example.com wrongpass").await);
        assert!(resp.last().unwrap().contains("NO"));
    }

    #[tokio::test]
    #[ignore]
    async fn select_inbox() {
        let mut session = test_session().await;
        session.handle_line("a001 LOGIN alice@example.com password123").await;
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
        session.handle_line("a001 LOGIN alice@example.com password123").await;
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
                "", "", "",
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
        session.handle_line("a001 LOGIN alice@example.com password123").await;
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
                "", "", "",
            )
            .await
            .unwrap();

        let resp = responses(session.handle_line("a003 STORE 1 +FLAGS (\\Seen)").await);
        let joined = resp.join("");
        assert!(joined.contains("\\Seen"));
        assert!(resp.last().unwrap().contains("OK"));

        // verify flag was persisted
        let mb = session.mailbox_store.get_mailbox("alice@example.com", "INBOX").await.unwrap().unwrap();
        let msg = session.mailbox_store.get_message(mb.id, 1).await.unwrap().unwrap();
        assert_eq!(msg.flags & FLAG_SEEN, FLAG_SEEN);
    }

    #[tokio::test]
    #[ignore]
    async fn list_default_mailboxes() {
        let mut session = test_session().await;
        session.handle_line("a001 LOGIN alice@example.com password123").await;
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
        session.handle_line("a001 LOGIN alice@example.com password123").await;
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
        session.handle_line("a001 LOGIN alice@example.com password123").await;
        session.handle_line("a002 SELECT INBOX").await;

        session.mailbox_store.index_message("alice@example.com", "INBOX", "msg001", "", "", "", 100, 1000, "", "", "").await.unwrap();
        session.mailbox_store.index_message("alice@example.com", "INBOX", "msg002", "", "", "", 200, 2000, "", "", "").await.unwrap();

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
        session.handle_line("a001 LOGIN alice@example.com password123").await;

        let result = session.handle_line("a002 APPEND INBOX {100}").await;
        match result {
            HandleResult::NeedLiteral { continuation, size } => {
                assert!(continuation.starts_with("+"));
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
        session.handle_line("a001 LOGIN alice@example.com password123").await;
        session.handle_line("a002 SELECT INBOX").await;

        session.mailbox_store.index_message("alice@example.com", "INBOX", "msg001", "", "", "", 100, 1000, "", "", "").await.unwrap();

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
        session.handle_line("a001 LOGIN alice@example.com password123").await;
        assert_eq!(session.idle_user(), Some("alice@example.com"));

        let result = session.handle_line("a002 IDLE").await;
        match result {
            HandleResult::EnterIdle { continuation, tag } => {
                assert!(continuation.contains("idling"));
                assert_eq!(tag, "a002");
            }
            _ => panic!("expected EnterIdle"),
        }
    }

    #[tokio::test]
    #[ignore]
    async fn idle_selected() {
        let mut session = test_session().await;
        session.handle_line("a001 LOGIN alice@example.com password123").await;
        session.handle_line("a002 SELECT INBOX").await;
        assert_eq!(session.idle_user(), Some("alice@example.com"));
        assert!(session.selected_mailbox_id().is_some());
    }
}
