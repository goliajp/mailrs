//! IMAP IDLE (RFC 2177) handler plus the public accessors the
//! session manager calls to push status updates to an idling
//! client.
//!
//! IDLE puts the connection into "wait for changes" mode; the
//! session manager polls [`ImapSession::idle_status_update`]
//! periodically and writes any non-empty `Vec<Vec<u8>>` to the
//! socket. `idle_user` and `selected_mailbox_id` let the manager
//! key event subscriptions to the right user / mailbox.

use mailrs_imap_proto::format_exists;

use super::{strs_to_bytes, HandleResult, ImapSession, ImapState};

impl ImapSession {
    pub(super) fn handle_idle(&self, tag: &str) -> HandleResult {
        if let Err(resp) = self.authenticated_username(tag) {
            return HandleResult::Responses(strs_to_bytes(resp));
        }
        HandleResult::EnterIdle {
            continuation: b"+ idling\r\n".to_vec(),
            tag: tag.to_string(),
        }
    }

    /// Username if the session is at least Authenticated.
    pub fn idle_user(&self) -> Option<&str> {
        match &self.state {
            ImapState::Authenticated { username } | ImapState::Selected { username, .. } => {
                Some(username.as_str())
            }
            ImapState::NotAuthenticated => None,
        }
    }

    /// Currently-selected mailbox id, if any.
    pub fn selected_mailbox_id(&self) -> Option<i64> {
        match &self.state {
            ImapState::Selected { mailbox, .. } => Some(mailbox.id),
            _ => None,
        }
    }

    /// Status-update payload for the selected mailbox — currently
    /// just `EXISTS <total>`. Returns empty if no mailbox is
    /// selected or the status query fails. Called by the session
    /// manager whenever it observes a relevant event for this
    /// user's selected mailbox.
    pub async fn idle_status_update(&self) -> Vec<Vec<u8>> {
        if let Some(mb_id) = self.selected_mailbox_id()
            && let Ok((total, _)) = self.mailbox_store.mailbox_status(mb_id).await
        {
            return strs_to_bytes(vec![format_exists(total)]);
        }
        Vec::new()
    }
}
