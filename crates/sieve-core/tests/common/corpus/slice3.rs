//! Slice 3 corpus — 33 new scripts pushing differential parity
//! toward the ckpt 4 → 5 trigger gate of 200/200.

use super::{CorpusRow, MSG_FOLDED, MSG_LONGHDR, MSG_QUOTEDNAME, MSG_SPAM};

pub(super) fn corpus() -> Vec<CorpusRow> {
    let msg_spam = MSG_SPAM;
    let msg_longhdr = MSG_LONGHDR;
    let msg_folded = MSG_FOLDED;
    let msg_quotedname = MSG_QUOTEDNAME;
    vec![
        // :matches glob edge cases
        (
            "matches_star_middle",
            r#"if header :matches "Subject" "spam*offer" { discard; }"#,
            msg_spam,
        ),
        (
            "matches_multi_stars",
            r#"if header :matches "From" "*alice*example*" { discard; }"#,
            msg_spam,
        ),
        (
            "matches_question_no_extra_char",
            r#"if header :matches "Subject" "spamoffer?" { discard; }"#,
            msg_spam,
        ),
        // address :all + multi-header
        (
            "address_all_list_to_match",
            r#"if address :all ["From", "To"] "bob@dest.com" { discard; }"#,
            msg_spam,
        ),
        (
            "address_all_list_no_match",
            r#"if address :all ["From", "To"] "carol@other.com" { discard; }"#,
            msg_spam,
        ),
        // size precise boundaries — msg_spam is 81 bytes
        (
            "size_over_below_boundary",
            "if size :over 80 { discard; }",
            msg_spam,
        ),
        (
            "size_over_at_boundary",
            "if size :over 81 { discard; }",
            msg_spam,
        ),
        // nested if 3 deep
        (
            "nested_if_3_deep_match",
            r#"require ["fileinto"];
               if header :contains "Subject" "spam" {
                 if header :contains "From" "alice" {
                   if exists "To" { fileinto "Triple"; }
                 }
               }"#,
            msg_spam,
        ),
        (
            "nested_if_3_deep_inner_miss",
            r#"require ["fileinto"];
               if header :contains "Subject" "spam" {
                 if header :contains "From" "alice" {
                   if exists "X-Missing" { fileinto "Triple"; }
                 }
               }"#,
            msg_spam,
        ),
        // header :is with string-list on both sides (cross product)
        (
            "header_is_list_left_match",
            r#"if header :is ["From", "To"] "bob@dest.com" { discard; }"#,
            msg_spam,
        ),
        (
            "header_is_list_both_sides",
            r#"if header :is ["From", "Subject"] ["meeting tomorrow", "spam offer"] { discard; }"#,
            msg_spam,
        ),
        // multi-action sequences
        (
            "multi_fileinto_sequence",
            r#"require ["fileinto"]; fileinto "A"; fileinto "B";"#,
            msg_spam,
        ),
        (
            "fileinto_then_explicit_keep",
            r#"require ["fileinto"]; fileinto "Backup"; keep;"#,
            msg_spam,
        ),
        // implicit keep when test fires but no action emitted
        (
            "if_test_true_empty_block",
            r#"if exists "Subject" { }"#,
            msg_spam,
        ),
        (
            "if_outer_true_inner_false_no_action",
            r#"if exists "Subject" {
                 if exists "X-Missing" { discard; }
               }"#,
            msg_spam,
        ),
        // long header value :contains (150+ chars)
        (
            "long_header_contains_needle",
            r#"if header :contains "Subject" "NEEDLE_TOKEN" { discard; }"#,
            msg_longhdr,
        ),
        (
            "long_header_contains_absent",
            r#"if header :contains "Subject" "ABSENT_PHRASE" { discard; }"#,
            msg_longhdr,
        ),
        // empty string comparisons
        (
            "is_subject_against_empty_string",
            r#"if header :is "Subject" "" { discard; }"#,
            msg_spam,
        ),
        (
            "is_missing_header_against_empty_string",
            r#"if header :is "X-Empty" "" { discard; }"#,
            msg_spam,
        ),
        // allof with nested not
        (
            "allof_not_and_exists_true",
            r#"if allof(not header :is "Subject" "newsletter", exists "From") { discard; }"#,
            msg_spam,
        ),
        (
            "allof_not_false_short_circuit",
            r#"if allof(not header :is "Subject" "spam offer", exists "From") { discard; }"#,
            msg_spam,
        ),
        // anyof 3 branches with only third true
        (
            "anyof_three_only_third_true",
            r#"if anyof(header :is "Subject" "x", header :is "Subject" "y", header :contains "Subject" "spam") { discard; }"#,
            msg_spam,
        ),
        (
            "anyof_three_all_false",
            r#"if anyof(header :is "Subject" "x", header :is "Subject" "y", header :is "Subject" "z") { discard; }"#,
            msg_spam,
        ),
        // address :localpart with quoted display name containing comma
        (
            "address_localpart_quoted_display_name",
            r#"if address :localpart "From" "alice" { discard; }"#,
            msg_quotedname,
        ),
        (
            "address_domain_quoted_display_name",
            r#"if address :domain "From" "example.com" { discard; }"#,
            msg_quotedname,
        ),
        // size :over 0 — true for any non-empty body
        (
            "size_over_zero",
            "if size :over 0 { discard; }",
            msg_spam,
        ),
        // folded header unfolds before :contains
        (
            "folded_header_contains_continuation",
            r#"if header :contains "Subject" "continues on next line" { discard; }"#,
            msg_folded,
        ),
        (
            "folded_header_contains_tab_continuation",
            r#"if header :contains "Subject" "with tab continuation" { discard; }"#,
            msg_folded,
        ),
        // misc safety nets
        (
            "case_insensitive_contains_upper",
            r#"if header :contains "Subject" "OFFER" { discard; }"#,
            msg_spam,
        ),
        (
            "exists_multi_all_missing",
            r#"if exists ["X-Foo", "X-Bar"] { discard; }"#,
            msg_spam,
        ),
        (
            "allof_anyof_combined",
            r#"if allof(anyof(header :is "Subject" "x", header :contains "Subject" "spam"), exists "From") { discard; }"#,
            msg_spam,
        ),
        (
            "if_no_match_then_top_level_keep",
            r#"if header :is "Subject" "no-match" { discard; } keep;"#,
            msg_spam,
        ),
    ]
}
