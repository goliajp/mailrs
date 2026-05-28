//! Slice 4 corpus part E — slice 4.3 expansion, categories DD-NN
//! (advanced glob, UTF-8 strings, many actions, multi top-level
//! if, deep nesting stress, multi-header filters, edge sizes,
//! various require, case-sensitivity coverage, comments in deep
//! positions, sieve syntax edges).

use super::{CorpusRow, MSG_SPAM};

pub(super) fn corpus() -> Vec<CorpusRow> {
    let msg_spam = MSG_SPAM;
    let msg_utf8: &[u8] = "\
From: 田中 <tanaka@example.jp>\r\n\
To: bob@dest.com\r\n\
Subject: こんにちは\r\n\
\r\n\
body\r\n"
        .as_bytes();
    let msg_listid: &[u8] = b"\
From: list@announce.example.com\r\n\
To: bob@dest.com\r\n\
Subject: announce\r\n\
List-Id: <weekly.announce.example.com>\r\n\
List-Unsubscribe: <https://example.com/unsubscribe>\r\n\
\r\n\
body\r\n";
    let msg_priority: &[u8] = b"\
From: ceo@important.com\r\n\
To: bob@dest.com\r\n\
Subject: urgent\r\n\
Priority: 1\r\n\
X-Priority: 1\r\n\
\r\n\
body\r\n";
    vec![
        // --- DD. Glob (:matches) advanced edges ---
        (
            "matches_pattern_with_only_chars",
            r#"if header :matches "Subject" "spam offer" { discard; }"#,
            msg_spam,
        ),
        (
            "matches_star_at_start_only",
            r#"if header :matches "Subject" "*offer" { discard; }"#,
            msg_spam,
        ),
        (
            "matches_star_at_end_only",
            r#"if header :matches "From" "Alice*" { discard; }"#,
            msg_spam,
        ),
        (
            "matches_consecutive_stars",
            r#"if header :matches "Subject" "**spam**" { discard; }"#,
            msg_spam,
        ),
        // --- EE. UTF-8 / non-ASCII in strings ---
        (
            "utf8_from_match_localpart",
            r#"if address :localpart "From" "tanaka" { discard; }"#,
            msg_utf8,
        ),
        (
            "utf8_subject_contains_japanese",
            r#"if header :contains "Subject" "こんにちは" { discard; }"#,
            msg_utf8,
        ),
        (
            "utf8_domain_jp",
            r#"if address :domain "From" "example.jp" { discard; }"#,
            msg_utf8,
        ),
        // --- FF. Many actions in one script ---
        (
            "five_action_chain",
            r#"require ["fileinto"];
               fileinto "A"; fileinto "B"; fileinto "C"; fileinto "D"; keep;"#,
            msg_spam,
        ),
        (
            "alternating_filein_redirect",
            r#"require ["fileinto"];
               fileinto "X"; redirect "x@example.com"; fileinto "Y";"#,
            msg_spam,
        ),
        // --- GG. Multiple top-level if statements ---
        (
            "two_independent_ifs_first_matches",
            r#"require ["fileinto"];
               if header :contains "Subject" "spam" { fileinto "Spam"; }
               if header :contains "From" "alice" { fileinto "FromAlice"; }"#,
            msg_spam,
        ),
        (
            "two_independent_ifs_neither_matches",
            r#"require ["fileinto"];
               if header :is "Subject" "no-match-1" { fileinto "A"; }
               if header :is "Subject" "no-match-2" { fileinto "B"; }"#,
            msg_spam,
        ),
        // --- HH. Deep allof/anyof nesting stress ---
        (
            "allof_four_branches",
            r#"if allof(
                 exists "From",
                 exists "To",
                 exists "Subject",
                 header :contains "Subject" "spam"
               ) { discard; }"#,
            msg_spam,
        ),
        (
            "anyof_four_branches_only_last_true",
            r#"if anyof(
                 header :is "Subject" "a",
                 header :is "Subject" "b",
                 header :is "Subject" "c",
                 header :contains "Subject" "spam"
               ) { discard; }"#,
            msg_spam,
        ),
        (
            "double_nested_not",
            r#"if not not header :contains "Subject" "spam" { discard; }"#,
            msg_spam,
        ),
        // --- II. Multi-header filter combinations ---
        (
            "listid_header_filter_match",
            r#"require ["fileinto"];
               if header :contains "List-Id" "announce" { fileinto "Announcements"; }"#,
            msg_listid,
        ),
        (
            "list_unsubscribe_present",
            r#"require ["fileinto"];
               if exists "List-Unsubscribe" { fileinto "Mailing-Lists"; }"#,
            msg_listid,
        ),
        (
            "priority_header_match",
            r#"require ["fileinto"];
               if header :is "X-Priority" "1" { fileinto "Urgent"; }"#,
            msg_priority,
        ),
        // --- JJ. Edge sizes ---
        (
            "size_under_with_kilo_match",
            "if size :under 1K { discard; }",
            msg_spam,
        ),
        (
            "size_over_zero_with_else",
            r#"if size :over 0 { discard; } else { keep; }"#,
            msg_spam,
        ),
        // --- KK. require with various extensions ---
        (
            "require_fileinto_and_imap4flags_unused",
            // imap4flags listed in require but not used — engine is
            // permissive on capability declaration so it should be
            // a no-op.
            r#"require ["fileinto", "imap4flags"]; keep;"#,
            msg_spam,
        ),
        (
            "require_with_subaddress_unused",
            r#"require ["subaddress"]; keep;"#,
            msg_spam,
        ),
        // --- LL. Case-sensitivity coverage ---
        (
            "is_lowercase_pattern_matches_uppercase",
            r#"if header :is "Subject" "spam offer" { discard; }"#,
            msg_spam,
        ),
        (
            "contains_mixed_case",
            r#"if header :contains "Subject" "SpAm" { discard; }"#,
            msg_spam,
        ),
        // --- MM. Comments in deep positions ---
        (
            "comment_before_require",
            r#"# top of script
               require ["fileinto"];
               keep;"#,
            msg_spam,
        ),
        (
            "comment_after_last_action",
            r#"keep; /* fin */"#,
            msg_spam,
        ),
        // --- NN. Sieve syntax edges — moved to slice4_g ---
    ]
}
