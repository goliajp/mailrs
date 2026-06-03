//! Slice 1/2 corpus — the original 32 rows seeded when the engine
//! was first built. Kept stable; new coverage goes in `slice3`.

use super::{CorpusRow, MSG_CLEAN, MSG_SPAM};

pub(super) fn corpus() -> Vec<CorpusRow> {
    let msg_spam = MSG_SPAM;
    let msg_clean = MSG_CLEAN;
    vec![
        ("explicit_keep", "keep;", msg_spam),
        ("explicit_discard", "discard;", msg_spam),
        (
            "fileinto",
            r#"require ["fileinto"]; fileinto "Junk";"#,
            msg_spam,
        ),
        (
            "header_is_spam",
            r#"if header :is "Subject" "spam offer" { discard; }"#,
            msg_spam,
        ),
        (
            "header_is_no_match",
            r#"if header :is "Subject" "nothing-here" { discard; }"#,
            msg_spam,
        ),
        (
            "header_contains_spam",
            r#"require ["fileinto"];
               if header :contains "Subject" "spam" { fileinto "Spam"; }
               else { keep; }"#,
            msg_spam,
        ),
        (
            "header_contains_clean",
            r#"require ["fileinto"];
               if header :contains "Subject" "spam" { fileinto "Spam"; }
               else { keep; }"#,
            msg_clean,
        ),
        ("size_over_small", "if size :over 1 { discard; }", msg_spam),
        (
            "size_under_huge",
            "if size :under 100K { discard; }",
            msg_spam,
        ),
        (
            "exists_subject",
            r#"if exists "Subject" { discard; }"#,
            msg_spam,
        ),
        // --- slice 2 additions ---
        (
            "header_matches_glob_star_prefix",
            r#"if header :matches "Subject" "*offer*" { discard; }"#,
            msg_spam,
        ),
        (
            "header_matches_glob_question_mark",
            r#"if header :matches "Subject" "spam ?ffer" { discard; }"#,
            msg_spam,
        ),
        (
            "not_invert_spam",
            r#"if not header :is "Subject" "nothing" { discard; }"#,
            msg_spam,
        ),
        (
            "not_invert_clean",
            r#"if not header :is "Subject" "meeting tomorrow" { discard; }"#,
            msg_clean,
        ),
        (
            "allof_two_true",
            r#"if allof(header :contains "Subject" "spam", exists "From") { discard; }"#,
            msg_spam,
        ),
        (
            "allof_one_false",
            r#"if allof(header :contains "Subject" "spam", header :is "From" "wrong") { discard; }"#,
            msg_spam,
        ),
        (
            "anyof_one_true",
            r#"if anyof(header :is "Subject" "no-match", header :contains "Subject" "spam") { discard; }"#,
            msg_spam,
        ),
        (
            "anyof_all_false",
            r#"if anyof(header :is "Subject" "no-match-1", header :is "Subject" "no-match-2") { discard; }"#,
            msg_spam,
        ),
        (
            "address_localpart_match",
            r#"if address :localpart "From" "alice" { discard; }"#,
            msg_spam,
        ),
        (
            "address_localpart_no_match",
            r#"if address :localpart "From" "carol" { discard; }"#,
            msg_spam,
        ),
        (
            "address_domain_match",
            r#"if address :domain "From" "example.com" { discard; }"#,
            msg_spam,
        ),
        (
            "address_to_list_localpart",
            r#"require ["fileinto"];
               if address :localpart "To" "bob" { fileinto "ToBob"; } else { keep; }"#,
            msg_spam,
        ),
        (
            "elsif_chain_first_match",
            r#"require ["fileinto"];
               if header :is "Subject" "no-match" { fileinto "A"; }
               elsif header :contains "Subject" "spam" { fileinto "Spam"; }
               elsif header :contains "Subject" "offer" { fileinto "Ads"; }
               else { keep; }"#,
            msg_spam,
        ),
        (
            "elsif_chain_else_branch",
            r#"require ["fileinto"];
               if header :is "Subject" "no-match" { fileinto "A"; }
               elsif header :is "Subject" "another-no" { fileinto "B"; }
               else { keep; }"#,
            msg_clean,
        ),
        (
            "stop_short_circuit",
            r#"if header :contains "Subject" "spam" { discard; stop; }
               keep;"#,
            msg_spam,
        ),
        (
            "redirect_then_keep_unguarded",
            r#"redirect "forward@example.com";"#,
            msg_spam,
        ),
        (
            "reject_with_reason",
            r#"require ["reject"]; reject "policy violation";"#,
            msg_spam,
        ),
        (
            "case_insensitive_is",
            r#"if header :is "Subject" "SPAM OFFER" { discard; }"#,
            msg_spam,
        ),
        (
            "exists_missing",
            r#"if exists "X-Spam-Score" { discard; }"#,
            msg_spam,
        ),
        (
            "exists_multi_present",
            r#"if exists ["From", "To", "Subject"] { discard; }"#,
            msg_spam,
        ),
        (
            "exists_multi_partial",
            r#"if exists ["From", "To", "X-Missing"] { discard; }"#,
            msg_spam,
        ),
        (
            "nested_if_inside_if",
            r#"require ["fileinto"];
               if header :contains "Subject" "spam" {
                 if header :is "From" "Alice <alice@example.com>" { fileinto "SpamFromAlice"; }
                 else { fileinto "Spam"; }
               }"#,
            msg_spam,
        ),
    ]
}
