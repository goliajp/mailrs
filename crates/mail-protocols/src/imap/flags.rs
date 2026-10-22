//! IMAP system flags and the Maildir flags a store keeps them as, and
//! INTERNALDATE.

use crate::backend::Flag;

pub fn flags_to_imap(flags: &[Flag]) -> String {
    flags
        .iter()
        .map(|f| match f {
            Flag::Seen => "\\Seen",
            Flag::Replied => "\\Answered",
            Flag::Flagged => "\\Flagged",
            Flag::Trashed => "\\Deleted",
            Flag::Draft => "\\Draft",
            Flag::Passed => "\\Answered",
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The system flags of a flag list; keywords are left out.
pub fn parse_imap_flags(s: &str) -> Vec<Flag> {
    let cleaned = s.trim_matches(|c| c == '(' || c == ')');
    cleaned
        .split_whitespace()
        .filter_map(|w| match w.to_uppercase().as_str() {
            "\\SEEN" => Some(Flag::Seen),
            "\\ANSWERED" => Some(Flag::Replied),
            "\\FLAGGED" => Some(Flag::Flagged),
            "\\DELETED" => Some(Flag::Trashed),
            "\\DRAFT" => Some(Flag::Draft),
            _ => None,
        })
        .collect()
}

pub(super) fn format_internal_date(epoch: i64) -> String {
    use chrono::{DateTime, Utc};
    match DateTime::<Utc>::from_timestamp(epoch, 0) {
        Some(dt) => dt.format("%d-%b-%Y %H:%M:%S +0000").to_string(),
        None => "01-Jan-1970 00:00:00 +0000".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_flags_reads_backslash_names() {
        let f = parse_imap_flags("(\\Seen \\Flagged)");
        assert!(f.contains(&Flag::Seen));
        assert!(f.contains(&Flag::Flagged));
        assert_eq!(f.len(), 2);
    }

    #[test]
    fn flags_to_imap_serialises_known() {
        let s = flags_to_imap(&[Flag::Seen, Flag::Flagged]);
        assert_eq!(s, "\\Seen \\Flagged");
    }
}
