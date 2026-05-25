//! POP3 TRANSACTION-state state-mutation commands: DELE, RSET, QUIT.

use super::{Pop3Session, Pop3State};

impl Pop3Session {
    pub(super) fn handle_dele(&mut self, arg: &str) -> Vec<String> {
        let Pop3State::Transaction {
            ref mut messages, ..
        } = self.state
        else {
            return vec!["-ERR not authenticated\r\n".into()];
        };

        let Ok(num) = arg.trim().parse::<usize>() else {
            return vec!["-ERR invalid message number\r\n".into()];
        };
        if num == 0 || num > messages.len() {
            return vec!["-ERR no such message\r\n".into()];
        }
        let m = &mut messages[num - 1];
        if m.deleted {
            return vec!["-ERR message already deleted\r\n".into()];
        }
        m.deleted = true;
        vec![format!("+OK message {num} deleted\r\n")]
    }

    pub(super) fn handle_rset(&mut self) -> Vec<String> {
        if let Pop3State::Transaction {
            ref mut messages, ..
        } = self.state
        {
            for m in messages.iter_mut() {
                m.deleted = false;
            }
            let (count, size) = self.stat_values();
            vec![format!("+OK {count} messages ({size} octets)\r\n")]
        } else {
            vec!["-ERR not authenticated\r\n".into()]
        }
    }

    pub(super) async fn handle_quit(&mut self) -> Vec<String> {
        // if in transaction state, actually delete marked messages
        if let Pop3State::Transaction {
            ref username,
            ref messages,
        } = self.state
        {
            let deleted_uids: Vec<u32> = messages
                .iter()
                .filter(|m| m.deleted)
                .map(|m| m.uid)
                .collect();

            if !deleted_uids.is_empty() {
                let mailboxes = self
                    .mailbox_store
                    .list_mailboxes(username)
                    .await
                    .unwrap_or_default();
                if let Some(inbox) = mailboxes.iter().find(|mb| mb.name == "INBOX") {
                    // mark messages with \Deleted flag
                    for uid in &deleted_uids {
                        let _ = self
                            .mailbox_store
                            .add_flags(inbox.id, *uid, mailrs_mailbox::FLAG_DELETED)
                            .await;
                    }
                    // expunge (permanently remove flagged messages)
                    let _ = self.mailbox_store.expunge(inbox.id).await;
                }
            }
        }
        self.state = Pop3State::Authorization;
        vec!["+OK bye\r\n".into()]
    }
}
