//! Maildir flags and keyword bits: the `:2,FLAGS` suffix.
//!
//! Split out of `lib.rs` on 2026-08-14, when adding keyword-bit support
//! took that file to 535 prod lines and the size gate refused the deploy.
//! Cut by subject rather than by line count — everything here is about
//! what a file name says about a message, and nothing here touches the
//! filesystem.

/// Standard Maildir flag, as defined by the Maildir specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Flag {
    /// `D` — draft message.
    Draft,
    /// `F` — flagged / starred.
    Flagged,
    /// `P` — passed / forwarded.
    Passed,
    /// `R` — replied to.
    Replied,
    /// `S` — seen / read.
    Seen,
    /// `T` — trashed (typically expunged at next IMAP EXPUNGE).
    Trashed,
}

impl Flag {
    /// Single-character representation used in the filename suffix.
    pub fn as_char(self) -> char {
        match self {
            Flag::Draft => 'D',
            Flag::Flagged => 'F',
            Flag::Passed => 'P',
            Flag::Replied => 'R',
            Flag::Seen => 'S',
            Flag::Trashed => 'T',
        }
    }

    /// Parse a single flag character; returns `None` for unknown letters.
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            'D' => Some(Flag::Draft),
            'F' => Some(Flag::Flagged),
            'P' => Some(Flag::Passed),
            'R' => Some(Flag::Replied),
            'S' => Some(Flag::Seen),
            'T' => Some(Flag::Trashed),
            _ => None,
        }
    }
}

/// Parse flags from the `":2,FLAGS"` suffix of a Maildir filename.
/// Returns a sorted, deduplicated `Vec<Flag>`.
pub fn parse_flags(info: &str) -> Vec<Flag> {
    // format: ":2,FLAGS" where FLAGS is a sorted string of flag chars
    let flags_str = info.strip_prefix(":2,").unwrap_or("");
    let mut flags: Vec<Flag> = flags_str.chars().filter_map(Flag::from_char).collect();
    flags.sort();
    flags.dedup();
    flags
}

/// Serialize flags to the `":2,FLAGS"` suffix format. Flags are sorted
/// and deduplicated for a canonical representation.
///
/// **Drops keyword bits.** Use [`serialize_flags_and_keywords`] when
/// rewriting a suffix that may carry them — see [`keywords_of`].
pub fn serialize_flags(flags: &[Flag]) -> String {
    serialize_flags_and_keywords(flags, &[])
}

/// The Maildir++ **keyword bits** in a `":2,FLAGS"` suffix.
///
/// Lowercase `a`–`z`, each mapped to a name by a `dovecot-keywords` file
/// beside the mail. They are not [`Flag`]s and are not modelled as such —
/// the enum is the six standard flags and their meaning is fixed, while a
/// keyword's meaning lives in a file — but a rewrite that does not carry
/// them through erases them.
///
/// That is not hypothetical: every write of read state rebuilds the suffix
/// from a bitmask, so without this the first time a message was marked
/// read it would lose every keyword on it.
///
/// An unknown *uppercase* letter is deliberately not a keyword. It is a
/// standard flag this crate has not implemented, and reading it as a
/// keyword would give it a meaning it does not have.
pub fn keywords_of(info: &str) -> Vec<char> {
    let mut out: Vec<char> = info
        .strip_prefix(":2,")
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_lowercase())
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Serialize flags **and** keyword bits into one canonical suffix:
/// standard flags first, in their own order, then the keyword letters.
pub fn serialize_flags_and_keywords(flags: &[Flag], keywords: &[char]) -> String {
    let mut sorted: Vec<Flag> = flags.to_vec();
    sorted.sort();
    sorted.dedup();
    let mut chars: String = sorted.iter().map(|f| f.as_char()).collect();
    let mut kw: Vec<char> = keywords
        .iter()
        .copied()
        .filter(|c| c.is_ascii_lowercase())
        .collect();
    kw.sort_unstable();
    kw.dedup();
    chars.extend(kw);
    format!(":2,{chars}")
}

/// Add a flag to an existing `:2,FLAGS` info string, returning the new
/// info string. No-op if `flag` is already present.
pub fn add_flag(info: &str, flag: Flag) -> String {
    let mut flags = parse_flags(info);
    if !flags.contains(&flag) {
        flags.push(flag);
    }
    serialize_flags(&flags)
}
