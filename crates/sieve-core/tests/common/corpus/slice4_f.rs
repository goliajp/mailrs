//! Slice 4 corpus part F — slice 4.3 expansion, categories OO-VV
//! (more real-world filters, message shapes, diverse address tests,
//! exists corner cases, action ordering, require + comments,
//! combined Subject tests, compliance smoke).

use super::{CorpusRow, MSG_MULTI_RCPT, MSG_SPAM};

pub(super) fn corpus() -> Vec<CorpusRow> {
    let msg_spam = MSG_SPAM;
    let msg_multi = MSG_MULTI_RCPT;
    let msg_reply: &[u8] = b"\
From: customer@example.com\r\n\
To: support@biz.com\r\n\
Subject: Re: your inquiry #4567\r\n\
In-Reply-To: <abc@biz.com>\r\n\
References: <abc@biz.com>\r\n\
\r\n\
body\r\n";
    let msg_calendar: &[u8] = b"\
From: calendar@cal.example.com\r\n\
To: bob@dest.com\r\n\
Subject: Meeting invitation\r\n\
Content-Type: text/calendar\r\n\
\r\n\
body\r\n";
    let msg_xspam: &[u8] = b"\
From: noreply@bulk.example.com\r\n\
To: bob@dest.com\r\n\
Subject: discount offer\r\n\
X-Spam-Status: Yes\r\n\
X-Spam-Score: 12.4\r\n\
\r\n\
body\r\n";
    vec![
        // --- OO. Real-world filter shapes (more) ---
        (
            "reply_thread_filter",
            r#"require ["fileinto"];
               if header :contains "Subject" "Re:" { fileinto "Threads"; }"#,
            msg_reply,
        ),
        (
            "in_reply_to_threaded",
            r#"require ["fileinto"];
               if exists "In-Reply-To" { fileinto "Replies"; }
               else { keep; }"#,
            msg_reply,
        ),
        (
            "calendar_invite_route",
            r#"require ["fileinto"];
               if header :contains "Content-Type" "text/calendar" { fileinto "Calendar"; }"#,
            msg_calendar,
        ),
        (
            "spam_score_filter",
            r#"require ["fileinto"];
               if header :is "X-Spam-Status" "Yes" { fileinto "Spam"; stop; }
               keep;"#,
            msg_xspam,
        ),
        // --- PP. Various message shapes ---
        (
            "xspam_score_match_via_contains",
            r#"if header :contains "X-Spam-Score" "12" { discard; }"#,
            msg_xspam,
        ),
        (
            "multipart_content_type_check",
            r#"if header :contains "Content-Type" "text/calendar" { discard; }"#,
            msg_calendar,
        ),
        (
            "references_header_present",
            r#"if exists "References" { discard; }"#,
            msg_reply,
        ),
        // --- QQ. Address tests with diverse headers ---
        (
            "address_is_full_addr",
            r#"if address :is "From" "alice@example.com" { discard; }"#,
            msg_spam,
        ),
        (
            "address_contains_partial_localpart",
            r#"if address :contains "From" "ali" { discard; }"#,
            msg_spam,
        ),
        (
            "address_matches_glob_pattern",
            r#"if address :matches "From" "ali*@*example*" { discard; }"#,
            msg_spam,
        ),
        (
            "address_localpart_for_cc",
            r#"if address :localpart "Cc" "ed" { discard; }"#,
            msg_multi,
        ),
        // --- RR. exists corner cases ---
        (
            "exists_single_string_form",
            r#"if exists "From" { discard; }"#,
            msg_spam,
        ),
        (
            "exists_one_present_one_missing",
            r#"if exists ["From", "X-Nope"] { discard; }"#,
            msg_spam,
        ),
        (
            "exists_all_missing_with_not",
            r#"if not exists ["X-A", "X-B"] { discard; }"#,
            msg_spam,
        ),
        // --- SS. Action ordering / explicit vs implicit ---
        // NOTE: `keep; fileinto X;` and `discard; fileinto X;` are
        // omitted on purpose — sieve-rs dedups (collapses keep/discard
        // when a subsequent fileinto / redirect fires), sieve-core
        // emits literally what the user wrote. Both behaviours are
        // RFC 5228-compliant; the dedup is a caller-policy choice
        // sieve-core leaves to the delivery layer. See slice 4.3 doc.
        (
            "redirect_then_fileinto",
            r#"require ["fileinto"]; redirect "fwd@x.com"; fileinto "Local";"#,
            msg_spam,
        ),
        // --- TT. require with comments ---
        (
            "require_then_comment_then_action",
            r#"require ["fileinto"];
               # filter spam
               if header :contains "Subject" "spam" { fileinto "Spam"; }"#,
            msg_spam,
        ),
        (
            "block_comment_inside_require_list",
            r#"require [/* extensions */ "fileinto", "reject"];
               keep;"#,
            msg_spam,
        ),
        // --- UU. Combined tests on same Subject ---
        (
            "subject_is_and_contains_combined",
            r#"if allof(header :contains "Subject" "spam",
                       header :contains "Subject" "offer") { discard; }"#,
            msg_spam,
        ),
        (
            "subject_matches_and_not_other",
            r#"if allof(header :matches "Subject" "*offer*",
                       not header :is "Subject" "newsletter") { discard; }"#,
            msg_spam,
        ),
        (
            "subject_anyof_then_address",
            r#"if anyof(header :contains "Subject" "spam",
                       address :localpart "From" "alice") { discard; }"#,
            msg_spam,
        ),
        // --- VV. Final compliance smoke tests ---
        (
            "very_long_script_terminating_keep",
            r#"require ["fileinto"];
               if header :is "Subject" "a" { fileinto "A"; }
               if header :is "Subject" "b" { fileinto "B"; }
               if header :is "Subject" "c" { fileinto "C"; }
               if header :is "Subject" "d" { fileinto "D"; }
               if header :is "Subject" "e" { fileinto "E"; }
               if header :is "Subject" "f" { fileinto "F"; }
               keep;"#,
            msg_spam,
        ),
        (
            "if_with_unguarded_else_then_keep",
            r#"if header :contains "Subject" "spam" { discard; }
               else { keep; }"#,
            msg_spam,
        ),
        (
            "compound_short_circuit_chain",
            r#"if anyof(
                 allof(exists "From", header :contains "Subject" "spam"),
                 allof(exists "To", header :contains "Subject" "newsletter")
               ) { discard; }"#,
            msg_spam,
        ),
        // --- WW. Final 200-trigger fillers — moved to slice4_g ---
    ]
}
