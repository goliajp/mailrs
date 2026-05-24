//! IMAP STORE and EXPUNGE handlers.
//!
//! STORE mutates per-message flags with optional UNCHANGEDSINCE
//! (RFC 7162) conditional update; EXPUNGE removes \Deleted
//! messages from the selected mailbox. Both require Selected
//! state.

use mailrs_imap_proto::{format_bad, format_no, format_ok, parse_sequence_set, sequence_set_to_uids};
use mailrs_imap_format::{format_imap_flags, parse_imap_flags};

use super::ImapSession;

impl ImapSession {
    pub(super) async fn handle_store(
        &self,
        tag: &str,
        sequence: &str,
        action: &str,
        flags_str: &str,
        use_uid: bool,
    ) -> Vec<String> {
        let mailbox = match self.selected_mailbox(tag) {
            Ok(mb) => mb,
            Err(resp) => return resp,
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
                    let modseq_str = flags_str.get(..paren_end).unwrap_or("").trim();
                    let rest = flags_str.get(paren_end + 1..).unwrap_or("").trim();
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
                && !real_action.contains(".SILENT")
            {
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

    pub(super) async fn handle_expunge(&self, tag: &str) -> Vec<String> {
        let mailbox = match self.selected_mailbox(tag) {
            Ok(mb) => mb,
            Err(resp) => return resp,
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
}
