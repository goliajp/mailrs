//! Slice 5 corpus part A — RFC 5232 `imap4flags` extension.
//! Covers setflag / addflag / removeflag commands, `:flags` tag
//! on fileinto / keep, hasflag test. All scripts include
//! `require ["imap4flags"]` per RFC 5232.

use super::{CorpusRow, MSG_SPAM};

pub(super) fn corpus() -> Vec<CorpusRow> {
    let msg_spam = MSG_SPAM;
    vec![
        // --- setflag — replace implicit flags variable ---
        (
            "setflag_single_then_keep",
            r#"require ["imap4flags"]; setflag "\\Seen"; keep;"#,
            msg_spam,
        ),
        (
            "setflag_list_then_keep",
            r#"require ["imap4flags"]; setflag ["\\Seen", "\\Important"]; keep;"#,
            msg_spam,
        ),
        // --- addflag — additive on top of current set ---
        (
            "addflag_after_setflag",
            r#"require ["imap4flags"];
               setflag "\\Seen";
               addflag "\\Important";
               keep;"#,
            msg_spam,
        ),
        (
            "addflag_list_on_empty",
            r#"require ["imap4flags"];
               addflag ["\\Seen", "$Label1"];
               keep;"#,
            msg_spam,
        ),
        // --- removeflag — subtractive ---
        (
            "removeflag_present",
            r#"require ["imap4flags"];
               setflag ["\\Seen", "\\Important"];
               removeflag "\\Seen";
               keep;"#,
            msg_spam,
        ),
        (
            "removeflag_missing",
            r#"require ["imap4flags"];
               setflag "\\Seen";
               removeflag "\\Answered";
               keep;"#,
            msg_spam,
        ),
        // --- fileinto with :flags tag override ---
        (
            "fileinto_with_flags_tag",
            r#"require ["fileinto", "imap4flags"];
               fileinto :flags "\\Seen" "Inbox";"#,
            msg_spam,
        ),
        (
            "fileinto_with_flags_list",
            r#"require ["fileinto", "imap4flags"];
               fileinto :flags ["\\Seen", "\\Important"] "Archive";"#,
            msg_spam,
        ),
        (
            "fileinto_picks_up_setflag",
            r#"require ["fileinto", "imap4flags"];
               setflag "\\Seen";
               fileinto "Inbox";"#,
            msg_spam,
        ),
        // --- keep with :flags ---
        (
            "keep_with_flags_tag",
            r#"require ["imap4flags"]; keep :flags "\\Seen";"#,
            msg_spam,
        ),
        (
            "keep_picks_up_setflag",
            r#"require ["imap4flags"]; setflag "\\Important"; keep;"#,
            msg_spam,
        ),
        // --- hasflag test ---
        (
            "hasflag_matches_set_flag",
            r#"require ["imap4flags"];
               setflag "\\Seen";
               if hasflag "\\Seen" { discard; }"#,
            msg_spam,
        ),
        (
            "hasflag_no_match_when_unset",
            r#"require ["imap4flags"];
               if hasflag "\\Seen" { discard; }"#,
            msg_spam,
        ),
        (
            "hasflag_list_any_match",
            r#"require ["imap4flags"];
               setflag "\\Important";
               if hasflag ["\\Seen", "\\Important"] { discard; }"#,
            msg_spam,
        ),
        // --- Compound: setflag inside if branch ---
        (
            "setflag_inside_if_then_fileinto",
            r#"require ["fileinto", "imap4flags"];
               if header :contains "Subject" "spam" {
                 setflag "\\Seen";
                 fileinto "Spam";
               }"#,
            msg_spam,
        ),
    ]
}
