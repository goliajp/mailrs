//! Flag parsing, ordering and the add/remove operations.

use crate::{Flag, add_flag, parse_flags, serialize_flags};

// --- Flag::as_char ---

#[test]
fn flag_as_char_all_variants() {
    assert_eq!(Flag::Draft.as_char(), 'D');
    assert_eq!(Flag::Flagged.as_char(), 'F');
    assert_eq!(Flag::Passed.as_char(), 'P');
    assert_eq!(Flag::Replied.as_char(), 'R');
    assert_eq!(Flag::Seen.as_char(), 'S');
    assert_eq!(Flag::Trashed.as_char(), 'T');
}
// --- Flag::from_char ---

#[test]
fn flag_from_char_all_valid() {
    assert_eq!(Flag::from_char('D'), Some(Flag::Draft));
    assert_eq!(Flag::from_char('F'), Some(Flag::Flagged));
    assert_eq!(Flag::from_char('P'), Some(Flag::Passed));
    assert_eq!(Flag::from_char('R'), Some(Flag::Replied));
    assert_eq!(Flag::from_char('S'), Some(Flag::Seen));
    assert_eq!(Flag::from_char('T'), Some(Flag::Trashed));
}

#[test]
fn flag_from_char_unknown_returns_none() {
    for c in ['d', 'f', 'p', 'r', 's', 't', 'X', '1', ' ', '\0'] {
        assert_eq!(Flag::from_char(c), None, "'{c}' should return None");
    }
}

#[test]
fn flag_roundtrip_char() {
    let all = [
        Flag::Draft,
        Flag::Flagged,
        Flag::Passed,
        Flag::Replied,
        Flag::Seen,
        Flag::Trashed,
    ];
    for flag in all {
        assert_eq!(Flag::from_char(flag.as_char()), Some(flag));
    }
}
// --- Flag ordering ---

#[test]
fn flag_ord_matches_char_order() {
    // Maildir spec requires flags to be stored in ASCII order: D < F < P < R < S < T
    assert!(Flag::Draft < Flag::Flagged);
    assert!(Flag::Flagged < Flag::Passed);
    assert!(Flag::Passed < Flag::Replied);
    assert!(Flag::Replied < Flag::Seen);
    assert!(Flag::Seen < Flag::Trashed);
}
// --- flag operations ---

#[test]
fn parse_flags_rs() {
    let flags = parse_flags(":2,RS");
    assert_eq!(flags, vec![Flag::Replied, Flag::Seen]);
}

#[test]
fn parse_flags_order() {
    // input in wrong order, output should be normalized
    let flags = parse_flags(":2,SR");
    assert_eq!(flags, vec![Flag::Replied, Flag::Seen]);
}

#[test]
fn parse_flags_all_six() {
    let flags = parse_flags(":2,DFPRST");
    assert_eq!(
        flags,
        vec![
            Flag::Draft,
            Flag::Flagged,
            Flag::Passed,
            Flag::Replied,
            Flag::Seen,
            Flag::Trashed,
        ]
    );
}

#[test]
fn parse_flags_deduplicates() {
    // duplicate chars in the info string must produce a single flag
    let flags = parse_flags(":2,SSR");
    assert_eq!(flags, vec![Flag::Replied, Flag::Seen]);
}

#[test]
fn parse_flags_ignores_unknown_chars() {
    // unknown chars must be silently skipped
    let flags = parse_flags(":2,SXR9");
    assert_eq!(flags, vec![Flag::Replied, Flag::Seen]);
}

#[test]
fn parse_flags_no_prefix() {
    // when the info string has no ":2," prefix the result should be empty
    let flags = parse_flags("RS");
    assert!(flags.is_empty(), "without :2, prefix flags must be empty");
}

#[test]
fn parse_flags_wrong_version() {
    // ":1," is not a valid flags section
    let flags = parse_flags(":1,RS");
    assert!(flags.is_empty());
}

#[test]
fn parse_no_flags() {
    let flags = parse_flags(":2,");
    assert!(flags.is_empty());
}

#[test]
fn serialize_flags_sorted() {
    let s = serialize_flags(&[Flag::Seen, Flag::Replied]);
    assert_eq!(s, ":2,RS");
}

#[test]
fn serialize_flags_empty() {
    let s = serialize_flags(&[]);
    assert_eq!(s, ":2,");
}

#[test]
fn serialize_flags_deduplicates() {
    // duplicate input flags must produce a single char each
    let s = serialize_flags(&[Flag::Seen, Flag::Seen, Flag::Draft]);
    assert_eq!(s, ":2,DS");
}

#[test]
fn serialize_flags_all() {
    let s = serialize_flags(&[
        Flag::Trashed,
        Flag::Seen,
        Flag::Replied,
        Flag::Passed,
        Flag::Flagged,
        Flag::Draft,
    ]);
    assert_eq!(s, ":2,DFPRST");
}

#[test]
fn parse_serialize_roundtrip() {
    let original = ":2,DRS";
    let flags = parse_flags(original);
    let serialized = serialize_flags(&flags);
    assert_eq!(serialized, original);
}

#[test]
fn add_flag_to_existing() {
    let result = add_flag(":2,S", Flag::Replied);
    assert_eq!(result, ":2,RS");
}

#[test]
fn add_flag_idempotent() {
    // adding a flag that already exists must not duplicate it
    let result = add_flag(":2,RS", Flag::Seen);
    assert_eq!(result, ":2,RS");
}

#[test]
fn add_flag_to_empty_info() {
    // starting from an empty info string
    let result = add_flag(":2,", Flag::Draft);
    assert_eq!(result, ":2,D");
}

#[test]
fn add_flag_to_no_prefix() {
    // info string without ":2," — parse_flags returns [], flag is added fresh
    let result = add_flag("", Flag::Flagged);
    assert_eq!(result, ":2,F");
}
