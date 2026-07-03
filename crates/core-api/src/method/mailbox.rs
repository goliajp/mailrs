//! Mailbox CRUD endpoints.
//!
//! Source: `crates/mailbox/src/pg/mailbox_ops.rs` (8 fn) +
//! `crates/mailbox/src/store.rs` (`MailboxStore` trait).

use serde::{Deserialize, Serialize};

use crate::types::{MailboxId, UserAddress};

// ── paths ───────────────────────────────────────────────────────────

pub const PATH_LIST_MAILBOXES: &str = "/v1/users/{user}/mailboxes";
pub const PATH_GET_MAILBOX: &str = "/v1/users/{user}/mailboxes/{name}";
pub const PATH_GET_MAILBOX_BY_ID: &str = "/v1/mailboxes/{id}";
pub const PATH_CREATE_MAILBOX: &str = "/v1/users/{user}/mailboxes";
pub const PATH_DELETE_MAILBOX: &str = "/v1/users/{user}/mailboxes/{name}";
pub const PATH_RENAME_MAILBOX: &str = "/v1/users/{user}/mailboxes/{name}/rename";
pub const PATH_MAILBOX_STATUS: &str = "/v1/mailboxes/{id}/status";
pub const PATH_ENSURE_DEFAULT: &str = "/v1/users/{user}/mailboxes:ensure-default";

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

/// Wire mirror of `mailrs_mailbox::types::MailboxStatus`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct MailboxStatusWire {
    /// Total message count.
    pub total: u32,
    /// Messages without `FLAG_SEEN` (and not analysed as spam/scam).
    pub unread: u32,
    /// IMAP `\Recent` count.
    pub recent: u32,
}

impl From<mailrs_mailbox::types::MailboxStatus> for MailboxStatusWire {
    fn from(s: mailrs_mailbox::types::MailboxStatus) -> Self {
        Self {
            total: s.total,
            unread: s.unread,
            recent: s.recent,
        }
    }
}

// ── req/resp ────────────────────────────────────────────────────────

/// Request body for `POST /v1/users/{user}/mailboxes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMailboxRequest {
    /// IMAP-safe mailbox name to create.
    pub name: String,
}

/// Response body — the new mailbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMailboxResponse {
    /// The created mailbox row.
    pub mailbox: MailboxWire,
}

/// Response body for `GET /v1/users/{user}/mailboxes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListMailboxesResponse {
    /// All of the user's mailboxes, ordered by name.
    pub items: Vec<MailboxWire>,
}

/// Request body for `POST /v1/users/{user}/mailboxes/{name}/rename`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameMailboxRequest {
    /// New mailbox name.
    pub to: String,
}

/// Response body for `GET /v1/mailboxes/{id}/status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxStatusResponse {
    /// Counts.
    pub status: MailboxStatusWire,
}

/// Response body for `POST /v1/users/{user}/mailboxes:ensure-default`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnsureDefaultMailboxesResponse {
    /// Number of mailboxes that were newly created (0 if all already existed).
    pub created: u32,
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

    #[test]
    fn mailbox_status_default() {
        let s = MailboxStatusWire::default();
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"total\":0"));
    }

    #[test]
    fn create_request_roundtrip() {
        let req = CreateMailboxRequest {
            name: "Archive".into(),
        };
        let s = serde_json::to_string(&req).unwrap();
        let back: CreateMailboxRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(back.name, "Archive");
    }
}
