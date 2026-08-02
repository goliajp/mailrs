//! The pure helpers, exercised without touching the filesystem.

use crate::{Flag, MessageId, add_flag, parse_flags, serialize_flags};

// --- additional pure function tests ---

#[test]
fn parse_flags_empty_string() {
    let flags = parse_flags("");
    assert!(flags.is_empty(), "empty string should yield no flags");
}

#[test]
fn parse_flags_colon_only() {
    let flags = parse_flags(":");
    assert!(flags.is_empty());
}

#[test]
fn parse_flags_colon_two_only() {
    let flags = parse_flags(":2");
    assert!(flags.is_empty());
}

#[test]
fn parse_flags_unicode_chars() {
    // unicode characters in the flags section should be ignored
    let flags = parse_flags(":2,S\u{00e9}R\u{00fc}");
    assert_eq!(flags, vec![Flag::Replied, Flag::Seen]);
}

#[test]
fn parse_flags_single_flag() {
    assert_eq!(parse_flags(":2,D"), vec![Flag::Draft]);
    assert_eq!(parse_flags(":2,F"), vec![Flag::Flagged]);
    assert_eq!(parse_flags(":2,P"), vec![Flag::Passed]);
    assert_eq!(parse_flags(":2,R"), vec![Flag::Replied]);
    assert_eq!(parse_flags(":2,S"), vec![Flag::Seen]);
    assert_eq!(parse_flags(":2,T"), vec![Flag::Trashed]);
}

#[test]
fn parse_flags_lowercase_ignored() {
    // lowercase versions of flag chars are not valid
    let flags = parse_flags(":2,dfprst");
    assert!(flags.is_empty());
}

#[test]
fn serialize_flags_single_flag_each() {
    assert_eq!(serialize_flags(&[Flag::Draft]), ":2,D");
    assert_eq!(serialize_flags(&[Flag::Flagged]), ":2,F");
    assert_eq!(serialize_flags(&[Flag::Passed]), ":2,P");
    assert_eq!(serialize_flags(&[Flag::Replied]), ":2,R");
    assert_eq!(serialize_flags(&[Flag::Seen]), ":2,S");
    assert_eq!(serialize_flags(&[Flag::Trashed]), ":2,T");
}

#[test]
fn serialize_flags_reverse_order_normalized() {
    // flags given in reverse order should still serialize in ASCII order
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
fn serialize_flags_with_many_duplicates() {
    let s = serialize_flags(&[Flag::Seen, Flag::Seen, Flag::Seen, Flag::Seen]);
    assert_eq!(s, ":2,S");
}

#[test]
fn add_flag_builds_correct_order() {
    // start empty, add flags in reverse order, verify sorted output
    let info = add_flag("", Flag::Trashed);
    let info = add_flag(&info, Flag::Draft);
    let info = add_flag(&info, Flag::Replied);
    assert_eq!(info, ":2,DRT");
}

#[test]
fn add_flag_all_flags_incrementally() {
    let mut info = String::from(":2,");
    for flag in [
        Flag::Seen,
        Flag::Draft,
        Flag::Flagged,
        Flag::Passed,
        Flag::Replied,
        Flag::Trashed,
    ] {
        info = add_flag(&info, flag);
    }
    assert_eq!(info, ":2,DFPRST");
}

#[test]
fn add_flag_idempotent_repeated() {
    // adding the same flag many times should be idempotent
    let mut info = ":2,S".to_string();
    for _ in 0..10 {
        info = add_flag(&info, Flag::Seen);
    }
    assert_eq!(info, ":2,S");
}

#[test]
fn parse_serialize_roundtrip_empty() {
    let original = ":2,";
    let flags = parse_flags(original);
    let serialized = serialize_flags(&flags);
    assert_eq!(serialized, original);
}

#[test]
fn parse_serialize_roundtrip_all_flags() {
    let original = ":2,DFPRST";
    let flags = parse_flags(original);
    let serialized = serialize_flags(&flags);
    assert_eq!(serialized, original);
}

#[test]
fn parse_serialize_roundtrip_single_flags() {
    for info in [":2,D", ":2,F", ":2,P", ":2,R", ":2,S", ":2,T"] {
        let flags = parse_flags(info);
        let serialized = serialize_flags(&flags);
        assert_eq!(serialized, info);
    }
}

#[test]
fn flag_clone_preserves_value() {
    let original = Flag::Seen;
    let cloned = original;
    assert_eq!(original, cloned);
}

#[test]
fn flag_debug_format() {
    // verify Debug is implemented and produces expected output
    let debug = format!("{:?}", Flag::Draft);
    assert_eq!(debug, "Draft");
    let debug = format!("{:?}", Flag::Seen);
    assert_eq!(debug, "Seen");
}

#[test]
fn message_id_clone() {
    let original = MessageId("test-id".to_string());
    let cloned = original.clone();
    assert_eq!(original, cloned);
    // verify they are independent copies
    assert_eq!(cloned.0, "test-id");
}

#[test]
fn message_id_debug_format() {
    let id = MessageId("123.M0P1Q2.host".to_string());
    let debug = format!("{:?}", id);
    assert!(debug.contains("123.M0P1Q2.host"));
}
