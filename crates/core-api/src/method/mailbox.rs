//! The user's mailbox list.

use serde::{Deserialize, Serialize};

use crate::types::{MailboxId, UserAddress};

// ── paths ───────────────────────────────────────────────────────────

pub const PATH_LIST_MAILBOXES: &str = "/v1/users/{user}/mailboxes";

// ── wire types ──────────────────────────────────────────────────────

/// Wire mirror of `mailrs_mailbox::types::Mailbox`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailboxWire {
    /// Store-native primary key.
    pub id: MailboxId,
    /// Owner email address.
    pub user: UserAddress,
    /// IMAP-safe collection name.
    pub name: String,
    /// IMAP UIDVALIDITY counter.
    pub uidvalidity: u32,
    /// IMAP UIDNEXT — UID for next insert.
    pub uidnext: u32,
    /// RFC 7162 CONDSTORE HIGHESTMODSEQ.
    pub highest_modseq: u64,
}

impl From<&mailrs_mailbox::types::Mailbox> for MailboxWire {
    fn from(m: &mailrs_mailbox::types::Mailbox) -> Self {
        Self {
            id: m.id,
            user: m.user.clone(),
            name: m.name.clone(),
            uidvalidity: m.uidvalidity,
            uidnext: m.uidnext,
            highest_modseq: m.highest_modseq,
        }
    }
}

impl From<mailrs_mailbox::types::Mailbox> for MailboxWire {
    fn from(m: mailrs_mailbox::types::Mailbox) -> Self {
        (&m).into()
    }
}

// ── req/resp ────────────────────────────────────────────────────────

/// Response body for `GET /v1/users/{user}/mailboxes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListMailboxesResponse {
    /// All of the user's mailboxes, ordered by name.
    pub items: Vec<MailboxWire>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_wire_roundtrip() {
        let w = MailboxWire {
            id: 7,
            user: "u@x.com".into(),
            name: "INBOX".into(),
            uidvalidity: 1,
            uidnext: 100,
            highest_modseq: 42,
        };
        let s = serde_json::to_string(&w).unwrap();
        let back: MailboxWire = serde_json::from_str(&s).unwrap();
        assert_eq!(w, back);
    }
}
