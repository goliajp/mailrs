//! Slice 4 corpus part G — slice 4.3 expansion overflow.
//! Carries the NN (sieve syntax edges) + WW (200-trigger fillers)
//! sub-groups that wouldn't fit in `slice4_e` / `slice4_f` under
//! the 200-line function limit.

use super::{CorpusRow, MSG_SPAM};

pub(super) fn corpus() -> Vec<CorpusRow> {
    let msg_spam = MSG_SPAM;
    vec![
        // --- NN. Sieve syntax edges (moved from slice4_e) ---
        (
            "extra_whitespace_in_test",
            r#"if   header   :is   "Subject"   "spam offer"   {  discard ;  }"#,
            msg_spam,
        ),
        (
            "newlines_inside_test_args",
            r#"if header
                  :is
                  "Subject"
                  "spam offer"
               { discard; }"#,
            msg_spam,
        ),
        (
            "string_list_with_one_element",
            r#"if exists ["From"] { discard; }"#,
            msg_spam,
        ),
        // --- WW. Final 200-trigger fillers (moved from slice4_f) ---
        (
            "elsif_after_anyof_falls_to_else_keep",
            r#"require ["fileinto"];
               if anyof(header :is "Subject" "x", header :is "Subject" "y") { fileinto "XY"; }
               elsif header :is "Subject" "z" { fileinto "Z"; }
               else { keep; }"#,
            msg_spam,
        ),
        (
            "redirect_with_long_address",
            r#"redirect "very.long.localpart+suffix@deeply.nested.subdomain.example.com";"#,
            msg_spam,
        ),
        (
            "size_chain_or_subject",
            r#"if anyof(size :over 100K, header :contains "Subject" "spam") { discard; }"#,
            msg_spam,
        ),
        (
            "fileinto_path_with_dots",
            r#"require ["fileinto"]; fileinto "Inbox.Sub.Deeper";"#,
            msg_spam,
        ),
        (
            "discard_inside_anyof_branch_via_if",
            r#"if anyof(header :contains "Subject" "spam") { discard; }"#,
            msg_spam,
        ),
        (
            "exists_then_address_combined",
            r#"if allof(exists "From", address :localpart "From" "alice") { discard; }"#,
            msg_spam,
        ),
        (
            "all_three_match_types_in_one_anyof",
            r#"if anyof(
                 header :is "Subject" "spam offer",
                 header :contains "Subject" "offer",
                 header :matches "Subject" "*offer*"
               ) { discard; }"#,
            msg_spam,
        ),
        (
            "address_is_localpart_then_domain",
            r#"if allof(address :localpart "From" "alice",
                       address :domain "From" "example.com") { discard; }"#,
            msg_spam,
        ),
        (
            "size_under_and_subject_present",
            r#"if allof(size :under 10K, exists "Subject") { discard; }"#,
            msg_spam,
        ),
        (
            "deeply_combined_with_elsif",
            r#"require ["fileinto"];
               if allof(exists "From", header :contains "Subject" "spam") {
                 if exists "To" { fileinto "Spam-Direct"; }
                 else { fileinto "Spam-Bcc"; }
               }
               elsif header :contains "Subject" "newsletter" { fileinto "Newsletters"; }
               else { keep; }"#,
            msg_spam,
        ),
    ]
}
