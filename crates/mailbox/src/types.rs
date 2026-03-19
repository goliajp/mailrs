use mailrs_storage_maildir::Flag;

/// mailbox metadata
#[derive(Debug, Clone)]
pub struct Mailbox {
    pub id: i64,
    pub user: String,
    pub name: String,
    pub uidvalidity: u32,
    pub uidnext: u32,
    pub highest_modseq: u64,
}

/// message metadata stored in PostgreSQL
#[derive(Debug, Clone)]
pub struct MessageMeta {
    pub id: i64,
    pub mailbox_id: i64,
    pub uid: u32,
    pub maildir_id: String,
    pub sender: String,
    pub recipients: String,
    pub subject: String,
    pub date: i64,
    pub size: u32,
    pub flags: u32,
    pub internal_date: i64,
    pub message_id: String,
    pub in_reply_to: String,
    pub thread_id: String,
    pub modseq: u64,
    /// owner's email address (for cross-domain queries)
    pub user_address: String,
    // importance fields (populated by post-delivery processing)
    pub importance_level: String,
    pub importance_score: f32,
    pub is_bulk_sender: bool,
    pub has_tracking_pixel: bool,
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

/// flag update action for CONDSTORE UNCHANGEDSINCE
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagAction {
    Set,
    Add,
    Remove,
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
