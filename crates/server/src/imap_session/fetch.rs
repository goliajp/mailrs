//! IMAP FETCH handler and the maildir file-reader it depends on.
//!
//! FETCH is the heaviest IMAP verb — it parses an attribute list,
//! filters the selected mailbox by sequence/UID set, and emits a
//! per-message response with any of: FLAGS, UID, RFC822.SIZE,
//! INTERNALDATE, ENVELOPE, MODSEQ, BODY[…], BODYSTRUCTURE.
//! `read_message_file` lives here because FETCH is its only caller.

use mailrs_imap_format::{
    build_bodystructure, extract_body_section, extract_header_fields, extract_header_section,
    extract_mime_part,
};
use mailrs_imap_proto::{
    format_bad, format_no, format_ok, parse_sequence_set, sequence_set_to_uids,
};
use mailrs_mailbox::{FLAG_SEEN, Mailbox, MessageMeta};

use super::{ImapSession, ImapState, strs_to_bytes};

mod items;

use items::{
    FetchAttrs, fetch_response_line, literal_item, metadata_items, requested_sequence_number,
};

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

        let attrs = FetchAttrs::parse(attributes, use_uid);

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
            let Some(seq_num) = requested_sequence_number(&messages, msg, &uids, use_uid) else {
                continue;
            };

            // CHANGEDSINCE filter: skip messages not modified since the given modseq
            if let Some(since) = attrs.changedsince
                && msg.modseq <= since
            {
                continue;
            }

            let mut items = metadata_items(msg, &attrs);

            if (attrs.any_body || attrs.bodystructure)
                && let Some(data) = self.read_message_file(msg).await
            {
                self.push_body_items(&mut items, mailbox, msg, &data, &attrs)
                    .await;
            }

            responses.push(fetch_response_line(seq_num, &items));
        }

        responses.push(format_ok(tag, "FETCH completed").into_bytes());
        responses
    }

    /// BODYSTRUCTURE and the BODY[...] sections, from the message's bytes.
    /// A full BODY[] or a non-PEEK generic section marks the message seen.
    async fn push_body_items(
        &self,
        items: &mut Vec<Vec<u8>>,
        mailbox: &Mailbox,
        msg: &MessageMeta,
        data: &[u8],
        attrs: &FetchAttrs,
    ) {
        if attrs.bodystructure {
            items.push(format!("BODYSTRUCTURE {}", build_bodystructure(data)).into_bytes());
        }
        // binary-safe literal builder: prefix + raw bytes
        if attrs.body_header {
            items.push(literal_item("BODY[HEADER]", &extract_header_section(data)));
        }
        if attrs.body_text {
            items.push(literal_item("BODY[TEXT]", &extract_body_section(data)));
        }
        if attrs.body_peek || attrs.body_full {
            items.push(literal_item("BODY[]", data));
            if attrs.body_full {
                let _ = self
                    .mailbox_store
                    .add_flags(mailbox.id, msg.uid, FLAG_SEEN)
                    .await;
            }
        }
        if let Some((ref fields, ref raw_section)) = attrs.header_fields_request {
            let filtered = extract_header_fields(data, fields);
            items.push(literal_item(&format!("BODY[{raw_section}]"), &filtered));
        }
        for section in &attrs.generic_body_sections {
            let part_data =
                extract_mime_part(data, section).unwrap_or_else(|| extract_body_section(data));
            items.push(literal_item(&format!("BODY[{section}]"), &part_data));
            if !attrs.peek {
                let _ = self
                    .mailbox_store
                    .add_flags(mailbox.id, msg.uid, FLAG_SEEN)
                    .await;
            }
        }
    }

    /// Read raw message bytes from the Maildir on disk for the
    /// current authenticated user. Tries a direct lookup by
    /// `maildir_id` in `new/` then `cur/` (with common flag
    /// suffixes), then falls back to scanning the directory.
    /// Returns `None` if the session isn't in Selected state or
    /// the file is missing.
    ///
    /// Async-aware: each disk read uses `tokio::fs::read` so the
    /// tokio worker isn't blocked on syscall latency. Under
    /// concurrent IMAP FETCH load this lets other tasks make
    /// progress while a slow seek completes.
    pub(super) async fn read_message_file(
        &self,
        msg: &mailrs_mailbox::MessageMeta,
    ) -> Option<Vec<u8>> {
        let username = match &self.state {
            ImapState::Selected { username, .. } => username,
            _ => return None,
        };
        let (local, domain) = username.split_once('@')?;
        let path = format!("{}/{domain}/{local}", self.maildir_root);

        // read through the MessageStore seam: fetch does the
        // scan-by-id + read on the blocking pool, replacing the old
        // brittle ":2,FRS"-suffix guessing + manual scan fallback.
        self.message_store
            .fetch(
                &path,
                &crate::message_store::MessageId(msg.maildir_id.clone()),
            )
            .await
            .ok()
            .flatten()
    }
}
