//! Slice 4 corpus part A — categories A-G (comments, escape
//! sequences, K/M/G suffix, elsif long chains, deep nesting,
//! multi-action, require multi-ext).

use super::{CorpusRow, MSG_CLEAN, MSG_QUOTED_SUBJECT, MSG_SPAM};

pub(super) fn corpus() -> Vec<CorpusRow> {
    let msg_spam = MSG_SPAM;
    let msg_clean = MSG_CLEAN;
    let msg_qsubj = MSG_QUOTED_SUBJECT;
    vec![
        // --- A. Comments (# line + /* block */) ---
        (
            "line_comment_before_action",
            "# comment line\nkeep;",
            msg_spam,
        ),
        (
            "block_comment_between_actions",
            r#"require ["fileinto"]; /* this is
               a multi-line comment */ fileinto "Inbox";"#,
            msg_spam,
        ),
        (
            "trailing_line_comment_after_semicolon",
            "discard; # trailing comment\n",
            msg_spam,
        ),
        // --- B. Escape sequences in quoted strings ---
        (
            "header_is_escaped_quote",
            r#"if header :is "Subject" "He said \"hi\"" { discard; }"#,
            msg_qsubj,
        ),
        (
            "header_contains_backslash_pattern",
            r#"if header :contains "Subject" "said \"hi" { discard; }"#,
            msg_qsubj,
        ),
        // --- C. Numbers with K/M/G suffix ---
        (
            "size_over_1k_msg_below",
            "if size :over 1K { discard; }",
            msg_spam,
        ),
        (
            "size_under_1m",
            "if size :under 1M { discard; }",
            msg_spam,
        ),
        (
            "size_under_2g",
            "if size :under 2G { discard; }",
            msg_spam,
        ),
        // --- D. elsif chains (long) ---
        (
            "elsif_5_levels_first_match",
            r#"require ["fileinto"];
               if header :is "Subject" "spam offer" { fileinto "Spam"; }
               elsif header :is "Subject" "x" { fileinto "X"; }
               elsif header :is "Subject" "y" { fileinto "Y"; }
               elsif header :is "Subject" "z" { fileinto "Z"; }
               else { keep; }"#,
            msg_spam,
        ),
        (
            "elsif_5_levels_last_branch",
            r#"require ["fileinto"];
               if header :is "Subject" "a" { fileinto "A"; }
               elsif header :is "Subject" "b" { fileinto "B"; }
               elsif header :is "Subject" "c" { fileinto "C"; }
               elsif header :is "Subject" "d" { fileinto "D"; }
               elsif header :contains "Subject" "spam" { fileinto "FoundSpam"; }
               else { keep; }"#,
            msg_spam,
        ),
        (
            "elsif_chain_falls_to_else",
            r#"require ["fileinto"];
               if header :is "Subject" "a" { fileinto "A"; }
               elsif header :is "Subject" "b" { fileinto "B"; }
               elsif header :is "Subject" "c" { fileinto "C"; }
               else { fileinto "Default"; }"#,
            msg_clean,
        ),
        // --- E. Deeply nested if (4+ levels) ---
        (
            "nested_if_4_deep_all_true",
            r#"require ["fileinto"];
               if exists "From" {
                 if exists "To" {
                   if exists "Subject" {
                     if header :contains "Subject" "spam" { fileinto "Quad"; }
                   }
                 }
               }"#,
            msg_spam,
        ),
        (
            "nested_if_4_deep_inner_false",
            r#"require ["fileinto"];
               if exists "From" {
                 if exists "To" {
                   if exists "Subject" {
                     if header :is "Subject" "no-such-subject" { fileinto "Quad"; }
                   }
                 }
               }"#,
            msg_spam,
        ),
        // --- F. Multi-action sequences ---
        (
            "fileinto_redirect_keep",
            r#"require ["fileinto"];
               fileinto "Archive";
               redirect "forward@example.com";
               keep;"#,
            msg_spam,
        ),
        (
            "two_redirects",
            r#"redirect "a@example.com"; redirect "b@example.com";"#,
            msg_spam,
        ),
        // --- G. require with multi-extension list ---
        (
            "require_multi_ext_list",
            r#"require ["fileinto", "reject", "envelope"];
               if header :contains "Subject" "spam" { reject "blocked"; }"#,
            msg_spam,
        ),
        (
            "require_repeated_calls",
            r#"require ["fileinto"];
               require ["reject"];
               fileinto "Junk";"#,
            msg_spam,
        ),
    ]
}
