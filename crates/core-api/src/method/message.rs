//! Message reads (by uid, by Message-ID, the invite it carries) and the
//! flag bitmask a message row uses.

use serde::{Deserialize, Serialize};

use crate::types::{MailboxId, MaildirId, MessageId, ThreadId, UserAddress};

// ── paths ───────────────────────────────────────────────────────────

/// Fastcore-native by-user variant — resolves the message via the per-user
/// uid → message_id index instead of a mailbox scan.
pub const PATH_GET_MESSAGE_BY_UID_USER: &str = "/v1/users/{user}/messages/by-uid/{uid}";
/// The invitation a message carries, typed, as stored at ingest.
///
/// Separate from the message read because it is a few kilobytes that
/// only an opened message needs, and because the row already says
/// whether there is one (`MessageWire::invite_method`) — a list can
/// mark every invite without fetching a single event.
pub const PATH_GET_INVITE: &str = "/v1/users/{user}/messages/by-uid/{uid}/invite";
pub const PATH_FIND_BY_MESSAGE_ID: &str = "/v1/users/{user}/messages/by-message-id/{message_id}";

/// What the bits of [`MessageWire::flags`] mean.
///
/// Re-exported from the stone that defines them rather than restated, so
/// both lanes and every consumer read one definition. Without this the
/// kevy lane cannot reach them at all — it does not depend on
/// `mailrs-mailbox` — and the alternative is what was there before: a
/// `let seen_bit = 0b0000_0001u32;` spelled out again at the call site,
/// which is how a bitmask ends up meaning two different things.
pub use mailrs_mailbox::types::{FLAG_DELETED, FLAG_FLAGGED, FLAG_SEEN};

/// Conversions between the bitmask and the maildir flag set.
///
/// **`bitmask_to_maildir_flags` is lossy in one direction and it matters
/// when writing.** Maildir has a `P` (passed / forwarded) flag with no bit
/// in this mask — `maildir_flags_to_bitmask` maps it to `0` — so
/// round-tripping a file's flags through the mask silently drops `P`. A
/// caller renaming a file to match a new bitmask must therefore start from
/// the flags already on the file and change only the bits the mask
/// represents, rather than replacing the set outright.
pub use mailrs_mailbox::types::{bitmask_to_maildir_flags, maildir_flags_to_bitmask};

// ── wire types ──────────────────────────────────────────────────────

/// Wire mirror of `mailrs_mailbox::types::Message`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageWire {
    /// **DEPRECATED under the fastcore/kevy-only architecture.** Was the
    /// SQL primary key of the PG-backed monolith. Fastcore has no
    /// equivalent primary key and always writes `id: 0`, which caused
    /// timeline duplication when the client used it as a React key
    /// (2026-07-08 bug). Every consumer (`ThreadMessageResponse`,
    /// `audit_message_raw`, JMAP bridge) now derives identity from
    /// (mailbox_id, uid) instead. The field is retained only for
    /// serde-compat with kevy blobs written before the fix; new writes
    /// still emit `0` and readers ignore it.
    #[serde(default)]
    pub id: MessageId,
    /// **DEPRECATED (v2.3.3 roadmap Phase 1.6).** FK into `mailboxes`
    /// under the PG monolith; fastcore keys messages by
    /// `(user_address, uid)` and has no numeric mailbox row. Frontend
    /// consumers (grep 2026-07-11: zero hits in `web/src`) never read
    /// this field; JMAP bridge derives from `user_address`. Retained
    /// as `#[serde(default)]` so fastcore emit sites can drop it and
    /// legacy readers still deserialize.
    #[serde(default)]
    pub mailbox_id: MailboxId,
    /// IMAP UID within mailbox_id.
    pub uid: u32,
    /// **DEPRECATED (v2.3.3 roadmap Phase 1.6).** Opaque body reference
    /// (maildir filename for the PG impl). Fastcore stores bodies by
    /// message-id hash into `mailrs:body:*` and does not populate this
    /// field. Zero frontend consumers; JMAP bridge writes its own
    /// blob ref. Retained with `#[serde(default)]` for legacy blob
    /// deserialization.
    #[serde(default)]
    pub blob_ref: MaildirId,
    /// Raw `From:` header.
    pub sender: String,
    /// Raw `To:` header (comma-separated).
    pub recipients: String,
    /// Decoded `Subject:`.
    pub subject: String,
    /// `Date:` epoch seconds.
    pub date: i64,
    /// Server-side delivery time, epoch seconds.
    pub internal_date: i64,
    /// Size in bytes.
    pub size: u32,
    /// Flag bitmask. The bit meanings are re-exported below as `FLAG_*`.
    pub flags: u32,
    /// RFC 5322 `Message-ID:` (no angle brackets).
    pub message_id: String,
    /// RFC 5322 `In-Reply-To:` (no angle brackets).
    pub in_reply_to: String,
    /// Sender-authentication verdict derived from the message's own
    /// `Authentication-Results` header at ingest: `"verified"` (DMARC
    /// pass), `"suspicious"` (an auth method failed — likely spoofed),
    /// `"unverified"` (auth ran but nothing conclusive), or `""` for
    /// messages ingested before this field existed. Self-hosted: it is
    /// the cryptographic auth result folded by
    /// `mailrs_inbound::sender_trust`, no model involved.
    #[serde(default)]
    pub sender_trust: String,
    /// iTIP method of the message's `text/calendar` part, upper-case
    /// (`REQUEST` / `REPLY` / `CANCEL` / `PUBLISH` / `COUNTER`), or
    /// empty for the overwhelming majority of mail, which carries no
    /// calendar part at all.
    ///
    /// Computed once at ingest, like [`Self::sender_trust`] beside it,
    /// and carried on every message read so a list can mark an invite
    /// without fetching one. The event itself is not here — it is a
    /// few kilobytes and only the opened message needs it — see
    /// `mailrs:invite:{message_id}`.
    #[serde(default)]
    pub invite_method: String,
    /// Resolved thread identifier.
    pub thread_id: ThreadId,
    /// CONDSTORE per-message MODSEQ.
    pub modseq: u64,
    /// Owner email address. Optional because some inherent methods return
    /// `MessageMeta` which doesn't carry it (cement handlers fill it from
    /// the request path when known, else leave blank).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user_address: UserAddress,
}

impl From<&mailrs_mailbox::types::MessageMeta> for MessageWire {
    fn from(m: &mailrs_mailbox::types::MessageMeta) -> Self {
        Self {
            id: m.id,
            mailbox_id: m.mailbox_id,
            uid: m.uid,
            blob_ref: m.maildir_id.clone(),
            sender: m.sender.clone(),
            recipients: m.recipients.clone(),
            subject: m.subject.clone(),
            date: m.date,
            internal_date: m.internal_date,
            size: m.size,
            flags: m.flags,
            message_id: m.message_id.clone(),
            in_reply_to: m.in_reply_to.clone(),
            // monolith (PG) path does not persist auth results yet;
            // empty means "no verdict", which renders as no badge.
            sender_trust: String::new(),
            // Neither PG-lane type carries it; empty means "no calendar
            // part", which renders as an ordinary message.
            invite_method: String::new(),
            thread_id: m.thread_id.clone(),
            modseq: m.modseq,
            // MessageMeta has no user_address — caller fills if needed.
            user_address: String::new(),
        }
    }
}

impl From<&mailrs_mailbox::types::Message> for MessageWire {
    fn from(m: &mailrs_mailbox::types::Message) -> Self {
        Self {
            id: m.id,
            mailbox_id: m.mailbox_id,
            uid: m.uid,
            blob_ref: m.blob_ref.clone(),
            sender: m.sender.clone(),
            recipients: m.recipients.clone(),
            subject: m.subject.clone(),
            date: m.date,
            internal_date: m.internal_date,
            size: m.size,
            flags: m.flags,
            message_id: m.message_id.clone(),
            in_reply_to: m.in_reply_to.clone(),
            // monolith (PG) path does not persist auth results yet;
            // empty means "no verdict", which renders as no badge.
            sender_trust: String::new(),
            // Neither PG-lane type carries it; empty means "no calendar
            // part", which renders as an ordinary message.
            invite_method: String::new(),
            thread_id: m.thread_id.clone(),
            modseq: m.modseq,
            user_address: m.user_address.clone(),
        }
    }
}

impl From<mailrs_mailbox::types::Message> for MessageWire {
    fn from(m: mailrs_mailbox::types::Message) -> Self {
        (&m).into()
    }
}
