//! IMAP UID dispatcher (RFC 3501 §6.4.8): forwards UID FETCH /
//! UID STORE / UID SEARCH / UID COPY / UID MOVE / UID SORT to
//! their non-UID handler counterparts with `use_uid = true`.
//!
//! UID SEARCH is the one case that doesn't trivially delegate —
//! it returns UIDs in the wire response instead of sequence
//! numbers — so it's implemented inline here.

use mailrs_imap_proto::{format_bad, format_no, format_ok, parse_search_criteria, ImapCommand};

use super::search::message_matches_criteria;
use super::{strs_to_bytes, ImapSession, ImapState};

impl ImapSession {
    pub(super) async fn handle_uid(
        &mut self,
        tag: &str,
        subcommand: &ImapCommand,
    ) -> Vec<Vec<u8>> {
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
            ImapCommand::Sort {
                criteria,
                search_criteria,
                ..
            } => strs_to_bytes(self.handle_sort(tag, criteria, search_criteria, true).await),
            _ => strs_to_bytes(vec![format_bad(tag, "unsupported UID subcommand")]),
        }
    }
}
