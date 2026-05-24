//! IMAP APPEND (RFC 3501 §6.3.11) handler — two-phase.
//!
//! `handle_append_start` validates the target mailbox + parses
//! the optional flags, stashes a [`PendingAppend`] on the
//! session, and returns [`HandleResult::NeedLiteral`] so the
//! session manager reads `literal_size` bytes from the wire.
//! `handle_literal_data` consumes that literal payload, calls
//! `PgMailboxStore::append_message` (which writes the Maildir
//! file + indexes metadata), and emits the
//! `[APPENDUID <uidvalidity> <uid>] APPEND completed` response
//! per RFC 4315.

use mailrs_imap_proto::{format_no, format_ok};
use mailrs_imap_format::parse_imap_flags;

use super::{strs_to_bytes, HandleResult, ImapSession, ImapState, PendingAppend};

impl ImapSession {
    pub(super) async fn handle_append_start(
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

    /// Consume APPEND literal bytes — called by the session
    /// manager after a previous [`HandleResult::NeedLiteral`].
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
