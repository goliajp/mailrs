//! IMAP COPY and MOVE handlers.
//!
//! COPY duplicates messages from the selected mailbox to a
//! destination by name; MOVE (RFC 6851) is COPY-then-EXPUNGE in
//! one verb and emits per-message `* <seq> EXPUNGE` untagged
//! responses on success. Both require Selected state and accept
//! sequence-set or UID input (the `use_uid` flag selects which).

use mailrs_imap_proto::{
    format_bad, format_no, format_ok, parse_sequence_set, sequence_set_to_uids,
};

use super::{ImapSession, ImapState};

impl ImapSession {
    pub(super) async fn handle_copy(
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

    pub(super) async fn handle_move(
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
}
