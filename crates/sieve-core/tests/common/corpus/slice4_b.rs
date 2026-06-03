//! Slice 4 corpus part B — categories H-O (empty blocks, nested
//! allof/anyof, multi-recipient address tests, case-insensitive
//! header lookup, mixed-case glob, address-part edges, reject
//! edges, stop short-circuit).

use super::{CorpusRow, MSG_MULTI_RCPT, MSG_SPAM};

pub(super) fn corpus() -> Vec<CorpusRow> {
    let msg_spam = MSG_SPAM;
    let msg_multi = MSG_MULTI_RCPT;
    vec![
        // --- H. Empty / minimal blocks ---
        (
            "empty_if_block_test_true",
            r#"if exists "Subject" { }
               keep;"#,
            msg_spam,
        ),
        (
            "empty_if_block_test_false",
            r#"if header :is "Subject" "no-match" { } keep;"#,
            msg_spam,
        ),
        // --- I. Nested allof/anyof combinations ---
        (
            "nested_allof_anyof_combined",
            r#"if allof(anyof(header :is "Subject" "x", header :contains "Subject" "spam"),
                       allof(exists "From", exists "To")) { discard; }"#,
            msg_spam,
        ),
        (
            "not_around_allof",
            r#"if not allof(header :is "Subject" "no-match", exists "From") { discard; }"#,
            msg_spam,
        ),
        (
            "not_around_anyof",
            r#"if not anyof(header :is "Subject" "no-match-1", header :is "Subject" "no-match-2") { discard; }"#,
            msg_spam,
        ),
        // --- J. Multi-recipient address tests ---
        (
            "address_to_multi_match",
            r#"if address :localpart "To" "carol" { discard; }"#,
            msg_multi,
        ),
        (
            "address_to_multi_no_match",
            r#"if address :localpart "To" "frank" { discard; }"#,
            msg_multi,
        ),
        (
            "address_cc_match",
            r#"if address :localpart "Cc" "ed" { discard; }"#,
            msg_multi,
        ),
        (
            "address_anyof_to_or_cc",
            r#"if anyof(address :localpart "To" "ed",
                       address :localpart "Cc" "ed") { discard; }"#,
            msg_multi,
        ),
        // --- K. Case-insensitive header lookup ---
        (
            "exists_lowercase_header_name",
            r#"if exists "subject" { discard; }"#,
            msg_spam,
        ),
        (
            "exists_mixed_case_header_list",
            r#"if exists ["SUBJECT", "to", "From"] { discard; }"#,
            msg_spam,
        ),
        (
            "header_is_lowercase_name",
            r#"if header :is "subject" "spam offer" { discard; }"#,
            msg_spam,
        ),
        // --- L. :matches glob with mixed-case pattern ---
        (
            "matches_uppercase_pattern",
            r#"if header :matches "Subject" "*OFFER*" { discard; }"#,
            msg_spam,
        ),
        // --- M. Address-part edges ---
        (
            "address_domain_exact_match",
            r#"if address :domain "From" "example.com" { discard; }"#,
            msg_spam,
        ),
        (
            "address_all_exact_match",
            r#"if address :all "From" "alice@example.com" { discard; }"#,
            msg_spam,
        ),
        (
            "address_localpart_case_insensitive",
            r#"if address :localpart "From" "ALICE" { discard; }"#,
            msg_spam,
        ),
        // --- N. Reject edge cases ---
        (
            "reject_short_reason",
            r#"require ["reject"]; reject "x";"#,
            msg_spam,
        ),
        (
            "reject_with_period_in_reason",
            r#"require ["reject"]; reject "blocked by policy.";"#,
            msg_spam,
        ),
        // --- O. stop short-circuit edge ---
        ("stop_at_top_level_before_keep", "stop; keep;", msg_spam),
    ]
}
