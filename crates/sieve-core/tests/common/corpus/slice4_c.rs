//! Slice 4 corpus part C — slice 4.2 expansion, categories P-V
//! (multi-line strings, number edges, whitespace tolerance,
//! header-value edges, address shape edges).

use super::{CorpusRow, MSG_SPAM};

pub(super) fn corpus() -> Vec<CorpusRow> {
    let msg_spam = MSG_SPAM;
    let msg_long_subject: &[u8] = b"\
From: Alice <alice@example.com>\r\n\
To: bob@dest.com\r\n\
Subject: \r\n\
\r\n\
body\r\n";
    let msg_dots_localpart: &[u8] = b"\
From: \"Alice S.\" <alice.smith@example.com>\r\n\
To: bob@dest.com\r\n\
Subject: hello\r\n\
\r\n\
body\r\n";
    let msg_subdomain: &[u8] = b"\
From: ops@sub.example.com\r\n\
To: bob@dest.com\r\n\
Subject: hello\r\n\
\r\n\
body\r\n";
    let msg_no_body: &[u8] = b"\
From: Alice <alice@example.com>\r\n\
To: bob@dest.com\r\n\
Subject: short\r\n\
\r\n";
    vec![
        // --- P. Multi-line `text:` strings in scripts ---
        (
            "multiline_reject_reason",
            "require [\"reject\"]; reject text:\nblocked by policy\n.\n;",
            msg_spam,
        ),
        (
            "multiline_fileinto_arg",
            "require [\"fileinto\"]; fileinto text:\nInbox\n.\n;",
            msg_spam,
        ),
        (
            "multiline_dot_stuffed_reason",
            "require [\"reject\"]; reject text:\n..stuffed line\n.\n;",
            msg_spam,
        ),
        // --- Q. Number edges ---
        ("size_under_one", "if size :under 1 { discard; }", msg_spam),
        (
            "size_over_huge",
            "if size :over 9999999999 { discard; }",
            msg_spam,
        ),
        (
            "size_over_exact_kilobyte",
            "if size :over 1024 { discard; }",
            msg_spam,
        ),
        // --- R. Whitespace tolerance ---
        (
            "many_blank_lines_between_actions",
            "keep;\n\n\n\n",
            msg_spam,
        ),
        (
            "mixed_tabs_and_spaces",
            "require\t[\"fileinto\"];\n  fileinto\t\"Junk\"  ;\n",
            msg_spam,
        ),
        (
            "actions_on_one_line",
            r#"require ["fileinto"]; fileinto "A"; fileinto "B"; keep;"#,
            msg_spam,
        ),
        // --- S. Header-value edges ---
        (
            "empty_subject_value_is_empty",
            r#"if header :is "Subject" "" { discard; }"#,
            msg_long_subject,
        ),
        (
            "empty_subject_contains_anything_false",
            r#"if header :contains "Subject" "anything" { discard; }"#,
            msg_long_subject,
        ),
        (
            "header_matches_empty_pattern",
            r#"if header :matches "Subject" "*" { discard; }"#,
            msg_spam,
        ),
        (
            "header_matches_only_question_mark",
            r#"if header :matches "Subject" "?" { discard; }"#,
            msg_spam,
        ),
        // --- T. Address shape edges ---
        (
            "address_localpart_with_dots",
            r#"if address :localpart "From" "alice.smith" { discard; }"#,
            msg_dots_localpart,
        ),
        (
            "address_domain_subdomain_match",
            r#"if address :domain "From" "sub.example.com" { discard; }"#,
            msg_subdomain,
        ),
        (
            "address_domain_parent_no_match",
            r#"if address :domain "From" "example.com" { discard; }"#,
            msg_subdomain,
        ),
        // --- U. Message shape edges ---
        (
            "no_body_size_zero",
            "if size :over 50 { discard; }",
            msg_no_body,
        ),
        (
            "no_body_exists_subject",
            r#"if exists "Subject" { discard; }"#,
            msg_no_body,
        ),
        // --- V. Comments in unusual positions ---
        (
            "comment_inside_block",
            r#"require ["fileinto"];
               if exists "Subject" {
                 # comment between if-test and action
                 fileinto "Has-Subject";
               }"#,
            msg_spam,
        ),
        (
            "block_comment_inside_test_list",
            r#"if allof(
                 /* first */ header :contains "Subject" "spam",
                 /* second */ exists "From"
               ) { discard; }"#,
            msg_spam,
        ),
    ]
}
