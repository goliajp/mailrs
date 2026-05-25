//! IMAP FETCH handler and the maildir file-reader it depends on.
//!
//! FETCH is the heaviest IMAP verb — it parses an attribute list,
//! filters the selected mailbox by sequence/UID set, and emits a
//! per-message response with any of: FLAGS, UID, RFC822.SIZE,
//! INTERNALDATE, ENVELOPE, MODSEQ, BODY[…], BODYSTRUCTURE.
//! `read_message_file` lives here because FETCH is its only caller.

use mailrs_imap_format::{
    build_bodystructure, extract_body_section, extract_header_fields, extract_header_section,
    extract_mime_part, format_addr_list, format_imap_flags, format_internal_date,
    parse_generic_body_sections, parse_header_fields_request, quote_or_nil,
};
use mailrs_imap_proto::{
    format_bad, format_no, format_ok, parse_sequence_set, sequence_set_to_uids,
};
use mailrs_mailbox::FLAG_SEEN;

use super::{ImapSession, ImapState, strs_to_bytes};

impl ImapSession {
    pub(super) async fn handle_fetch(
        &self,
        tag: &str,
        sequence: &str,
        attributes: &str,
        use_uid: bool,
    ) -> Vec<Vec<u8>> {
        let mailbox = match self.selected_mailbox(tag) {
            Ok(mb) => mb,
            Err(resp) => return strs_to_bytes(resp),
        };

        let seq_set = match parse_sequence_set(sequence) {
            Ok(s) => s,
            Err(e) => {
                return strs_to_bytes(vec![format_bad(tag, &format!("invalid sequence: {e}"))]);
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
            let after = attributes.get(pos + "CHANGEDSINCE".len()..).unwrap_or("");
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
            if let Some(since) = changedsince
                && msg.modseq <= since
            {
                continue;
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

            if (want_any_body || want_bodystructure)
                && let Some(data) = self.read_message_file(msg)
            {
                if want_bodystructure {
                    items
                        .push(format!("BODYSTRUCTURE {}", build_bodystructure(&data)).into_bytes());
                }
                // binary-safe literal builder: prefix + raw bytes
                if want_body_header {
                    let header = extract_header_section(&data);
                    let mut item = format!("BODY[HEADER] {{{}}}\r\n", header.len()).into_bytes();
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

    /// Read raw message bytes from the Maildir on disk for the
    /// current authenticated user. Tries a direct lookup by
    /// `maildir_id` in `new/` then `cur/` (with common flag
    /// suffixes), then falls back to scanning the directory.
    /// Returns `None` if the session isn't in Selected state or
    /// the file is missing.
    pub(super) fn read_message_file(&self, msg: &mailrs_mailbox::MessageMeta) -> Option<Vec<u8>> {
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
        let md = mailrs_maildir::Maildir::open(&base);
        let find_in = |entries: Vec<mailrs_maildir::Entry>| -> Option<Vec<u8>> {
            entries
                .into_iter()
                .find(|e| e.id.to_string() == msg.maildir_id)
                .and_then(|e| std::fs::read(&e.path).ok())
        };
        find_in(md.scan_cur().unwrap_or_default())
            .or_else(|| find_in(md.scan_new().unwrap_or_default()))
    }
}
