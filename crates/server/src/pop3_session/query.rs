//! POP3 TRANSACTION-state read commands: LIST, UIDL, STAT,
//! and the underlying `load_inbox` / `stat_values` helpers.

use super::{MessageEntry, Pop3Session, Pop3State};

impl Pop3Session {
    pub(super) async fn load_inbox(&self, user: &str) -> Vec<MessageEntry> {
        let mailboxes = self
            .mailbox_store
            .list_mailboxes(user)
            .await
            .unwrap_or_default();

        let inbox = mailboxes.iter().find(|mb| mb.name == "INBOX");
        let Some(inbox) = inbox else {
            return Vec::new();
        };

        let messages = self
            .mailbox_store
            .list_messages(inbox.id, 0, 10000)
            .await
            .unwrap_or_default();

        messages
            .into_iter()
            .map(|m| MessageEntry {
                uid: m.uid,
                maildir_id: m.maildir_id,
                size: m.size,
                deleted: false,
            })
            .collect()
    }

    pub(super) fn stat_values(&self) -> (usize, u64) {
        if let Pop3State::Transaction { ref messages, .. } = self.state {
            let active: Vec<&MessageEntry> = messages.iter().filter(|m| !m.deleted).collect();
            let size: u64 = active.iter().map(|m| m.size as u64).sum();
            (active.len(), size)
        } else {
            (0, 0)
        }
    }

    pub(super) fn handle_stat(&self) -> Vec<String> {
        if !matches!(self.state, Pop3State::Transaction { .. }) {
            return vec!["-ERR not authenticated\r\n".into()];
        }
        let (count, size) = self.stat_values();
        vec![format!("+OK {count} {size}\r\n")]
    }

    pub(super) fn handle_list(&self, arg: &str) -> Vec<String> {
        let Pop3State::Transaction { ref messages, .. } = self.state else {
            return vec!["-ERR not authenticated\r\n".into()];
        };

        if arg.is_empty() {
            // list all
            let (count, size) = self.stat_values();
            let mut resp = vec![format!("+OK {count} messages ({size} octets)\r\n")];
            for (i, m) in messages.iter().enumerate() {
                if !m.deleted {
                    resp.push(format!("{} {}\r\n", i + 1, m.size));
                }
            }
            resp.push(".\r\n".into());
            resp
        } else {
            // list specific message
            let Ok(num) = arg.trim().parse::<usize>() else {
                return vec!["-ERR invalid message number\r\n".into()];
            };
            if num == 0 || num > messages.len() {
                return vec!["-ERR no such message\r\n".into()];
            }
            let m = &messages[num - 1];
            if m.deleted {
                return vec!["-ERR message deleted\r\n".into()];
            }
            vec![format!("+OK {} {}\r\n", num, m.size)]
        }
    }

    pub(super) fn handle_uidl(&self, arg: &str) -> Vec<String> {
        let Pop3State::Transaction { ref messages, .. } = self.state else {
            return vec!["-ERR not authenticated\r\n".into()];
        };

        if arg.is_empty() {
            let mut resp = vec!["+OK\r\n".into()];
            for (i, m) in messages.iter().enumerate() {
                if !m.deleted {
                    resp.push(format!("{} {}\r\n", i + 1, m.uid));
                }
            }
            resp.push(".\r\n".into());
            resp
        } else {
            let Ok(num) = arg.trim().parse::<usize>() else {
                return vec!["-ERR invalid message number\r\n".into()];
            };
            if num == 0 || num > messages.len() {
                return vec!["-ERR no such message\r\n".into()];
            }
            let m = &messages[num - 1];
            if m.deleted {
                return vec!["-ERR message deleted\r\n".into()];
            }
            vec![format!("+OK {} {}\r\n", num, m.uid)]
        }
    }
}
