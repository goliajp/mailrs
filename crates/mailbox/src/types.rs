use mailrs_maildir::Flag;

/// Mailbox metadata.
///
/// Marked `#[non_exhaustive]` so the 1.x line can grow fields (e.g.
/// `subscribed`, `attributes`) without breaking downstream pattern matches.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Mailbox {
    /// Store-native primary key.
    pub id: i64,
    /// Owner email address.
    pub user: String,
    /// URL-/IMAP-safe collection name (e.g. `INBOX`, `Sent`).
    pub name: String,
    /// IMAP UIDVALIDITY counter. Stable for the lifetime of the mailbox;
    /// changes only when UIDs are reset.
    pub uidvalidity: u32,
    /// IMAP UIDNEXT — the UID that will be assigned to the next inserted
    /// message.
    pub uidnext: u32,
    /// RFC 7162 CONDSTORE HIGHESTMODSEQ — bumps on every flag change or
    /// message insert.
    pub highest_modseq: u64,
}

/// Per-mailbox counts surfaced by IMAP STATUS / JMAP `totalEmails`+`unreadEmails`.
#[derive(Debug, Clone, Copy, Default)]
pub struct MailboxStatus {
    /// Total message count.
    pub total: u32,
    /// Messages without `FLAG_SEEN`.
    pub unread: u32,
    /// IMAP `\Recent` count. Best-effort — implementations that don't track
    /// per-session recency should return 0.
    pub recent: u32,
}

/// A message's portable metadata — the fields every IMAP / JMAP backend
/// must expose.
///
/// Marked `#[non_exhaustive]` so the 1.x line can grow fields without
/// breaking downstream pattern matches. Backend-specific projections
/// (importance scoring, content rendering caches, etc.) live as separate
/// types on the concrete impl, NOT on this struct.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Message {
    /// Store-native primary key.
    pub id: i64,
    /// FK into the message's containing mailbox.
    pub mailbox_id: i64,
    /// IMAP UID within `mailbox_id`. Stable; never reused within a single
    /// UIDVALIDITY epoch.
    pub uid: u32,
    /// Opaque reference to the message body. The store impl chooses the
    /// format (e.g. maildir filename, blob-store key, file path). The
    /// library does not interpret this string.
    pub blob_ref: String,
    /// Raw `From:` header value (may include display name).
    pub sender: String,
    /// Raw `To:` header value. Comma-separated address list.
    pub recipients: String,
    /// Decoded `Subject:` header.
    pub subject: String,
    /// `Date:` header epoch seconds.
    pub date: i64,
    /// Server-side delivery time, epoch seconds.
    pub internal_date: i64,
    /// Message size in bytes.
    pub size: u32,
    /// Flag bitmask. See the [`FLAG_*`](crate::types::FLAG_SEEN) constants.
    pub flags: u32,
    /// RFC 5322 `Message-ID:` header value, without angle brackets.
    pub message_id: String,
    /// RFC 5322 `In-Reply-To:` header value, without angle brackets, or
    /// empty when the message is not a reply.
    pub in_reply_to: String,
    /// Store-resolved thread identifier, stable across all messages in the
    /// same conversation.
    pub thread_id: String,
    /// RFC 7162 CONDSTORE per-message MODSEQ.
    pub modseq: u64,
    /// Owner email address (for cross-domain queries).
    pub user_address: String,
}

/// Input to [`MailboxStore::insert_message`](crate::store::MailboxStore::insert_message).
///
/// Non-owning struct; caller keeps the strings alive across the call.
#[derive(Debug, Clone)]
pub struct InsertMessage<'a> {
    /// Owner email address.
    pub user: &'a str,
    /// Target mailbox name (e.g. `INBOX`).
    pub mailbox_name: &'a str,
    /// Opaque body reference (see [`Message::blob_ref`]).
    pub blob_ref: &'a str,
    /// Raw `From:` header value.
    pub sender: &'a str,
    /// Raw `To:` header value.
    pub recipients: &'a str,
    /// Decoded `Subject:` header.
    pub subject: &'a str,
    /// Message size in bytes.
    pub size: u32,
    /// `Date:` header epoch seconds.
    pub date: i64,
    /// Server-side delivery time, epoch seconds. Typically `now()`.
    pub internal_date: i64,
    /// RFC 5322 `Message-ID:` value, without angle brackets.
    pub message_id: &'a str,
    /// RFC 5322 `In-Reply-To:` value, without angle brackets.
    pub in_reply_to: &'a str,
    /// Thread identifier, resolved by the caller via
    /// [`crate::threading::resolve_thread_id`] or equivalent.
    pub thread_id: &'a str,
    /// Initial flag bitmask. IMAP APPEND can set; default to 0.
    pub flags: u32,
}

/// Result of a successful [`MailboxStore::insert_message`](crate::store::MailboxStore::insert_message).
#[derive(Debug, Clone, Copy)]
pub struct Inserted {
    /// Store-native primary key of the newly-inserted message.
    pub id: i64,
    /// Allocated UID within the target mailbox.
    pub uid: u32,
    /// Resulting MODSEQ after the insert.
    pub modseq: u64,
}

/// JMAP `Email/query`-shape filter for [`MailboxStore::query_messages`](crate::store::MailboxStore::query_messages).
///
/// Narrow by design — five fields cover the 80% case (mailbox scope, free
/// text, keyword filter, pagination). Richer filtering belongs at the
/// protocol layer above the store.
#[derive(Debug, Clone, Default)]
pub struct QueryFilter<'a> {
    /// Restrict to a single mailbox, or `None` for all user's mailboxes.
    pub mailbox_id: Option<i64>,
    /// Owner email address — required when `mailbox_id` is `None`.
    pub user: Option<&'a str>,
    /// Case-insensitive substring match across sender + recipients + subject.
    pub text: Option<&'a str>,
    /// Require this flag bit to be set.
    pub has_keyword: Option<u32>,
    /// Require this flag bit to be UNSET.
    pub not_keyword: Option<u32>,
    /// Pagination offset (0-based).
    pub position: u32,
    /// Page size. Implementations may cap; recommended sane default 50.
    pub limit: u32,
}

/// Flag mutation operation for CONDSTORE compare-and-swap
/// (see [`MailboxStore::store_flags_if_unchanged`](crate::store::MailboxStore::store_flags_if_unchanged)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagOp {
    /// Replace the bitmask entirely.
    Set,
    /// OR `flags` into the current bitmask.
    Add,
    /// AND-NOT `flags` out of the current bitmask.
    Remove,
}

/// Back-compat alias for [`FlagOp`]. Will be removed in 2.0.
pub type FlagAction = FlagOp;

/// **Legacy: PostgreSQL-impl-specific** extended message metadata.
///
/// Contains mailrs-internal projection fields (importance scoring, bulk-sender
/// flag, tracking-pixel flag, preview snippet) that are NOT part of the
/// portable [`MailboxStore`](crate::store::MailboxStore) trait. Returned by
/// the PG impl's legacy methods. Will be reshaped during the 2b refactor;
/// new code should use the trait's [`Message`] type and fetch any extension
/// data separately via PG-EXT inherent methods.
#[derive(Debug, Clone)]
pub struct MessageMeta {
    /// Store-native primary key.
    pub id: i64,
    /// FK into the message's containing mailbox.
    pub mailbox_id: i64,
    /// IMAP UID.
    pub uid: u32,
    /// Maildir filename (mailrs-specific blob reference).
    pub maildir_id: String,
    /// Raw `From:` header value.
    pub sender: String,
    /// Raw `To:` header value.
    pub recipients: String,
    /// Decoded `Subject:` header.
    pub subject: String,
    /// `Date:` header epoch seconds.
    pub date: i64,
    /// Message size in bytes.
    pub size: u32,
    /// Flag bitmask.
    pub flags: u32,
    /// Server-side delivery time epoch seconds.
    pub internal_date: i64,
    /// `Message-ID:` header, without angle brackets.
    pub message_id: String,
    /// `In-Reply-To:` header, without angle brackets.
    pub in_reply_to: String,
    /// Resolved thread identifier.
    pub thread_id: String,
    /// CONDSTORE per-message MODSEQ.
    pub modseq: u64,
    /// owner's email address (for cross-domain queries)
    pub user_address: String,
    // importance fields (populated by post-delivery processing)
    /// mailrs-internal importance bucket
    pub importance_level: String,
    /// mailrs-internal importance score [0.0, 1.0]
    pub importance_score: f32,
    /// mailrs-internal bulk-sender heuristic
    pub is_bulk_sender: bool,
    /// mailrs-internal tracking-pixel detection
    pub has_tracking_pixel: bool,
    /// mailrs-internal preview snippet
    pub new_content: Option<String>,
}

/// summary of a conversation thread
#[derive(Debug, Clone)]
pub struct ConversationSummary {
    pub thread_id: String,
    pub subject: String,
    pub participants: String,
    pub message_count: u32,
    pub unread_count: u32,
    pub last_date: i64,
    pub category: String,
    /// whether any message in the thread has FLAG_FLAGGED set
    pub flagged: bool,
    /// short preview of the latest message body
    pub snippet: String,
    /// whether this thread has been pinned by the user
    pub pinned: bool,
    /// whether this thread has been archived by the user
    pub archived: bool,
    /// highest importance level in the thread
    pub importance_level: String,
    /// highest importance score in the thread
    pub importance_score: f32,
    /// whether any message in the thread requires action
    pub requires_action: bool,
    /// sender of the most recent message in the thread (used client-side
    /// to hide "sent by me" threads from the default inbox view)
    pub last_sender: String,
    /// number of messages in the thread that live in the Sent mailbox
    /// — i.e. things the user sent themselves. The UI uses this together
    /// with `message_count` to render "x received · y sent" on the card.
    pub sent_count: u32,
}

/// AI analysis result stored in email_analysis table
#[derive(Debug, Clone)]
pub struct EmailAnalysisRow {
    pub message_id: i64,
    pub category: String,
    pub risk_score: i16,
    pub risk_reason: String,
    pub summary: String,
    pub people: serde_json::Value,
    pub dates: serde_json::Value,
    pub amounts: serde_json::Value,
    pub action_items: serde_json::Value,
    pub model_version: String,
    pub clean_text: String,
    pub requires_action: bool,
    pub sender_intent: String,
    pub action_deadline: Option<String>,
}

// flag bitmask constants
pub const FLAG_SEEN: u32 = 0b0000_0001;
pub const FLAG_ANSWERED: u32 = 0b0000_0010;
pub const FLAG_FLAGGED: u32 = 0b0000_0100;
pub const FLAG_DELETED: u32 = 0b0000_1000;
pub const FLAG_DRAFT: u32 = 0b0001_0000;
pub const FLAG_RECENT: u32 = 0b0010_0000;

/// convert maildir flags to bitmask
pub fn maildir_flags_to_bitmask(flags: &[Flag]) -> u32 {
    let mut bits = 0u32;
    for flag in flags {
        bits |= match flag {
            Flag::Seen => FLAG_SEEN,
            Flag::Replied => FLAG_ANSWERED,
            Flag::Flagged => FLAG_FLAGGED,
            Flag::Trashed => FLAG_DELETED,
            Flag::Draft => FLAG_DRAFT,
            Flag::Passed => 0,
        };
    }
    bits
}

/// convert bitmask to maildir flags
pub fn bitmask_to_maildir_flags(bits: u32) -> Vec<Flag> {
    let mut flags = Vec::new();
    if bits & FLAG_SEEN != 0 {
        flags.push(Flag::Seen);
    }
    if bits & FLAG_ANSWERED != 0 {
        flags.push(Flag::Replied);
    }
    if bits & FLAG_FLAGGED != 0 {
        flags.push(Flag::Flagged);
    }
    if bits & FLAG_DELETED != 0 {
        flags.push(Flag::Trashed);
    }
    if bits & FLAG_DRAFT != 0 {
        flags.push(Flag::Draft);
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_conversion_roundtrip() {
        let flags = vec![Flag::Seen, Flag::Replied, Flag::Flagged];
        let bits = maildir_flags_to_bitmask(&flags);
        assert_eq!(bits, FLAG_SEEN | FLAG_ANSWERED | FLAG_FLAGGED);

        let back = bitmask_to_maildir_flags(bits);
        assert!(back.contains(&Flag::Seen));
        assert!(back.contains(&Flag::Replied));
        assert!(back.contains(&Flag::Flagged));
        assert_eq!(back.len(), 3);
    }

    #[test]
    fn empty_flags() {
        assert_eq!(maildir_flags_to_bitmask(&[]), 0);
        assert!(bitmask_to_maildir_flags(0).is_empty());
    }

    #[test]
    fn single_flag_roundtrip() {
        for (flag, expected_bit) in [
            (Flag::Seen, FLAG_SEEN),
            (Flag::Replied, FLAG_ANSWERED),
            (Flag::Flagged, FLAG_FLAGGED),
            (Flag::Trashed, FLAG_DELETED),
            (Flag::Draft, FLAG_DRAFT),
        ] {
            let bits = maildir_flags_to_bitmask(&[flag]);
            assert_eq!(bits, expected_bit);
            let back = bitmask_to_maildir_flags(bits);
            assert_eq!(back.len(), 1);
            assert_eq!(back[0], flag);
        }
    }

    #[test]
    fn passed_flag_maps_to_zero() {
        assert_eq!(maildir_flags_to_bitmask(&[Flag::Passed]), 0);
    }

    #[test]
    fn all_flags_combined() {
        let all = vec![
            Flag::Seen,
            Flag::Replied,
            Flag::Flagged,
            Flag::Trashed,
            Flag::Draft,
            Flag::Passed,
        ];
        let bits = maildir_flags_to_bitmask(&all);
        assert_eq!(
            bits,
            FLAG_SEEN | FLAG_ANSWERED | FLAG_FLAGGED | FLAG_DELETED | FLAG_DRAFT
        );
        let back = bitmask_to_maildir_flags(bits);
        assert_eq!(back.len(), 5); // Passed not included
    }

    #[test]
    fn duplicate_flags_idempotent() {
        let flags = vec![Flag::Seen, Flag::Seen, Flag::Seen];
        let bits = maildir_flags_to_bitmask(&flags);
        assert_eq!(bits, FLAG_SEEN);
    }

    #[test]
    fn bitmask_ignores_unknown_bits() {
        // bits beyond defined flags should produce no extra flags
        let bits = 0b1111_1111;
        let flags = bitmask_to_maildir_flags(bits);
        assert_eq!(flags.len(), 5); // only 5 known flags
    }

    #[test]
    fn flag_action_variants() {
        assert_ne!(FlagAction::Set, FlagAction::Add);
        assert_ne!(FlagAction::Add, FlagAction::Remove);
        assert_ne!(FlagAction::Set, FlagAction::Remove);
    }

    #[test]
    fn flag_constants_are_powers_of_two() {
        assert_eq!(FLAG_SEEN.count_ones(), 1);
        assert_eq!(FLAG_ANSWERED.count_ones(), 1);
        assert_eq!(FLAG_FLAGGED.count_ones(), 1);
        assert_eq!(FLAG_DELETED.count_ones(), 1);
        assert_eq!(FLAG_DRAFT.count_ones(), 1);
        assert_eq!(FLAG_RECENT.count_ones(), 1);
    }

    #[test]
    fn flag_constants_no_overlap() {
        let all = FLAG_SEEN | FLAG_ANSWERED | FLAG_FLAGGED | FLAG_DELETED | FLAG_DRAFT | FLAG_RECENT;
        assert_eq!(all.count_ones(), 6);
    }

    #[test]
    fn bitmask_to_flags_recent_not_included() {
        // FLAG_RECENT is not mapped to a maildir flag
        let flags = bitmask_to_maildir_flags(FLAG_RECENT);
        assert!(flags.is_empty());
    }
}
