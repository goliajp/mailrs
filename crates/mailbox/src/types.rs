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
}
