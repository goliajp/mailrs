//! Slice 4 corpus part D — slice 4.2 expansion, categories W-Z+
//! (deep nesting variants, action sequence semantics, require
//! edges, RFC compliance specifics, combined real-world filters).

use super::{CorpusRow, MSG_SPAM};

pub(super) fn corpus() -> Vec<CorpusRow> {
    let msg_spam = MSG_SPAM;
    vec![
        // --- W. Deep nesting variants ---
        (
            "allof_three_branches_short_circuit",
            r#"if allof(header :is "Subject" "no-match", exists "From", exists "To") { discard; }"#,
            msg_spam,
        ),
        (
            "anyof_with_nested_not",
            r#"if anyof(not exists "Subject", header :contains "Subject" "spam") { discard; }"#,
            msg_spam,
        ),
        (
            "not_around_size",
            "if not size :over 100K { discard; }",
            msg_spam,
        ),
        (
            "if_inside_anyof_via_test",
            r#"if anyof(allof(exists "From", exists "To"),
                       header :is "Subject" "no-match") { discard; }"#,
            msg_spam,
        ),
        // --- X. Action sequence semantics ---
        (
            "fileinto_keep_explicit",
            r#"require ["fileinto"]; fileinto "Junk"; keep;"#,
            msg_spam,
        ),
        ("discard_alone", "discard;", msg_spam),
        (
            "stop_inside_else_branch",
            r#"if header :is "Subject" "no-match" { keep; }
               else { stop; }
               # this keep should never run because of stop
               keep;"#,
            msg_spam,
        ),
        (
            "stop_inside_then_branch_blocks_outer_actions",
            r#"if header :contains "Subject" "spam" {
                 stop;
               }
               discard;"#,
            msg_spam,
        ),
        // --- Y. require edges ---
        (
            "require_then_no_action",
            r#"require ["fileinto"];"#,
            msg_spam,
        ),
        (
            "require_only_keep",
            r#"require ["fileinto"];
               keep;"#,
            msg_spam,
        ),
        (
            "require_two_lists_combined",
            r#"require ["fileinto"];
               require ["reject", "vacation"];
               if header :contains "Subject" "spam" { reject "no"; }"#,
            msg_spam,
        ),
        // --- Z. RFC compliance specifics ---
        (
            "header_is_with_leading_space_in_value",
            r#"if header :is "Subject" "spam offer" { discard; }"#,
            msg_spam,
        ),
        (
            "address_localpart_uppercase_message",
            // The From address has lowercase, pattern is uppercase →
            // i;ascii-casemap default → match.
            r#"if address :localpart "From" "ALICE" { discard; }"#,
            msg_spam,
        ),
        (
            "exists_then_not_combined",
            r#"if allof(exists "From", not exists "X-Missing") { discard; }"#,
            msg_spam,
        ),
        (
            "elsif_after_anyof_else",
            r#"require ["fileinto"];
               if anyof(header :is "Subject" "x", header :is "Subject" "y") { fileinto "XY"; }
               elsif header :contains "Subject" "spam" { fileinto "Spam"; }
               else { keep; }"#,
            msg_spam,
        ),
        // --- AA. Real-world filter shapes ---
        (
            "newsletter_filter_pattern",
            r#"require ["fileinto"];
               if anyof(
                 header :contains "List-Id" "newsletter",
                 header :matches "Subject" "*[NEWSLETTER]*"
               ) { fileinto "Newsletters"; stop; }
               if header :contains "Subject" "spam" { fileinto "Spam"; }"#,
            msg_spam,
        ),
        (
            "vip_priority_pattern",
            r#"require ["fileinto"];
               if address :is "From" "vip@important.com" { fileinto "VIP"; stop; }
               if header :contains "Subject" "spam" { fileinto "Spam"; }
               else { keep; }"#,
            msg_spam,
        ),
        (
            "auto_archive_old_threads",
            r#"require ["fileinto"];
               if allof(
                 header :contains "Subject" "Re:",
                 size :over 50K
               ) { fileinto "Archive"; }
               else { keep; }"#,
            msg_spam,
        ),
        // --- BB. exists with non-existent in middle of list ---
        (
            "exists_middle_missing",
            r#"if exists ["From", "X-Missing", "To"] { discard; }"#,
            msg_spam,
        ),
        // --- CC. trivial scripts ---
        ("empty_script_implicit_keep", "", msg_spam),
        (
            "comment_only_script",
            "# this script does nothing useful\n",
            msg_spam,
        ),
    ]
}
