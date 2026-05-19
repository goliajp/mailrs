//! IMAP wire-format response formatters.
//!
//! All formatters return the complete line including the trailing `\r\n`.
//! Untagged responses (`* CAPABILITY ...`) start with `* `; tagged
//! responses (`a001 OK ...`) start with the caller-provided tag.

/// `* CAPABILITY <caps>` — response to `CAPABILITY` or untagged greeting.
pub fn format_capability(capabilities: &[&str]) -> String {
    format!("* CAPABILITY {}\r\n", capabilities.join(" "))
}

/// format LIST response line
pub fn format_list(flags: &str, delimiter: &str, name: &str) -> String {
    format!("* LIST ({flags}) \"{delimiter}\" {name}\r\n")
}

/// format FETCH response line
pub fn format_fetch(seq: u32, items: &[(String, String)]) -> String {
    let parts: Vec<String> = items
        .iter()
        .map(|(k, v)| format!("{k} {v}"))
        .collect();
    format!("* {seq} FETCH ({parts})\r\n", parts = parts.join(" "))
}

/// format OK tagged response
pub fn format_ok(tag: &str, message: &str) -> String {
    format!("{tag} OK {message}\r\n")
}

/// format NO tagged response
pub fn format_no(tag: &str, message: &str) -> String {
    format!("{tag} NO {message}\r\n")
}

/// format BAD tagged response
pub fn format_bad(tag: &str, message: &str) -> String {
    format!("{tag} BAD {message}\r\n")
}

/// format BYE untagged response
pub fn format_bye(message: &str) -> String {
    format!("* BYE {message}\r\n")
}

/// format FLAGS untagged response
pub fn format_flags(flags: &[&str]) -> String {
    format!("* FLAGS ({})\r\n", flags.join(" "))
}

/// format EXISTS untagged response
pub fn format_exists(count: u32) -> String {
    format!("* {count} EXISTS\r\n")
}

/// format RECENT untagged response
pub fn format_recent(count: u32) -> String {
    format!("* {count} RECENT\r\n")
}

/// format QUOTA response (RFC 2087) — usage and limit in KB
pub fn format_quota(quotaroot: &str, usage_kb: u64, limit_kb: u64) -> String {
    format!("* QUOTA \"{quotaroot}\" (STORAGE {usage_kb} {limit_kb})\r\n")
}

/// format QUOTAROOT response (RFC 2087)
pub fn format_quotaroot(mailbox: &str, quotaroot: &str) -> String {
    format!("* QUOTAROOT \"{mailbox}\" \"{quotaroot}\"\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_capability_response() {
        let result = format_capability(&["IMAP4rev1", "IDLE", "AUTH=PLAIN"]);
        assert_eq!(result, "* CAPABILITY IMAP4rev1 IDLE AUTH=PLAIN\r\n");
    }

    #[test]
    fn format_list_response() {
        let result = format_list("\\HasNoChildren", "/", "INBOX");
        assert_eq!(result, "* LIST (\\HasNoChildren) \"/\" INBOX\r\n");
    }

    #[test]
    fn format_ok_response() {
        assert_eq!(format_ok("a001", "LOGIN completed"), "a001 OK LOGIN completed\r\n");
    }

    #[test]
    fn format_no_response() {
        assert_eq!(format_no("a001", "LOGIN failed"), "a001 NO LOGIN failed\r\n");
    }

    #[test]
    fn format_bad_response() {
        assert_eq!(
            format_bad("a001", "Unknown command"),
            "a001 BAD Unknown command\r\n"
        );
    }

    #[test]
    fn format_bye_response() {
        assert_eq!(format_bye("Server shutting down"), "* BYE Server shutting down\r\n");
    }

    #[test]
    fn format_exists_response() {
        assert_eq!(format_exists(42), "* 42 EXISTS\r\n");
    }

    #[test]
    fn format_fetch_single_item() {
        let items = vec![("FLAGS".to_string(), "(\\Seen)".to_string())];
        assert_eq!(format_fetch(1, &items), "* 1 FETCH (FLAGS (\\Seen))\r\n");
    }

    #[test]
    fn format_fetch_multiple_items() {
        let items = vec![
            ("FLAGS".to_string(), "(\\Seen)".to_string()),
            ("UID".to_string(), "42".to_string()),
        ];
        assert_eq!(
            format_fetch(3, &items),
            "* 3 FETCH (FLAGS (\\Seen) UID 42)\r\n"
        );
    }

    #[test]
    fn format_flags_response() {
        let flags = &["\\Seen", "\\Answered", "\\Flagged"];
        assert_eq!(
            format_flags(flags),
            "* FLAGS (\\Seen \\Answered \\Flagged)\r\n"
        );
    }

    #[test]
    fn format_flags_empty() {
        let flags: &[&str] = &[];
        assert_eq!(format_flags(flags), "* FLAGS ()\r\n");
    }

    #[test]
    fn format_recent_response() {
        assert_eq!(format_recent(0), "* 0 RECENT\r\n");
        assert_eq!(format_recent(5), "* 5 RECENT\r\n");
    }

    #[test]
    fn format_quota_response() {
        assert_eq!(
            format_quota("user.alice", 1024, 10240),
            "* QUOTA \"user.alice\" (STORAGE 1024 10240)\r\n"
        );
    }

    #[test]
    fn format_quotaroot_response() {
        assert_eq!(
            format_quotaroot("INBOX", "user.alice"),
            "* QUOTAROOT \"INBOX\" \"user.alice\"\r\n"
        );
    }

    // --- additional edge-case tests ---

    #[test]
    fn format_capability_empty_list() {
        assert_eq!(format_capability(&[]), "* CAPABILITY \r\n");
    }

    #[test]
    fn format_capability_single_item() {
        assert_eq!(format_capability(&["IMAP4rev1"]), "* CAPABILITY IMAP4rev1\r\n");
    }

    #[test]
    fn format_fetch_empty_items() {
        let items: Vec<(String, String)> = vec![];
        assert_eq!(format_fetch(5, &items), "* 5 FETCH ()\r\n");
    }

    #[test]
    fn format_fetch_seq_zero() {
        let items = vec![("UID".to_string(), "100".to_string())];
        assert_eq!(format_fetch(0, &items), "* 0 FETCH (UID 100)\r\n");
    }

    #[test]
    fn format_exists_zero() {
        assert_eq!(format_exists(0), "* 0 EXISTS\r\n");
    }

    #[test]
    fn format_exists_large() {
        assert_eq!(format_exists(100_000), "* 100000 EXISTS\r\n");
    }

    #[test]
    fn format_recent_large() {
        assert_eq!(format_recent(9999), "* 9999 RECENT\r\n");
    }

    #[test]
    fn format_quota_zero_usage() {
        assert_eq!(
            format_quota("", 0, 0),
            "* QUOTA \"\" (STORAGE 0 0)\r\n"
        );
    }

    #[test]
    fn format_quota_large_values() {
        assert_eq!(
            format_quota("root", 1_000_000, 2_000_000),
            "* QUOTA \"root\" (STORAGE 1000000 2000000)\r\n"
        );
    }

    #[test]
    fn format_quotaroot_empty_strings() {
        assert_eq!(
            format_quotaroot("", ""),
            "* QUOTAROOT \"\" \"\"\r\n"
        );
    }

    #[test]
    fn format_list_multiple_flags() {
        let result = format_list("\\HasChildren \\Subscribed", "/", "INBOX");
        assert_eq!(result, "* LIST (\\HasChildren \\Subscribed) \"/\" INBOX\r\n");
    }

    #[test]
    fn format_list_empty_flags() {
        let result = format_list("", "/", "INBOX");
        assert_eq!(result, "* LIST () \"/\" INBOX\r\n");
    }

    #[test]
    fn format_ok_empty_message() {
        assert_eq!(format_ok("A", ""), "A OK \r\n");
    }

    #[test]
    fn format_no_empty_message() {
        assert_eq!(format_no("A", ""), "A NO \r\n");
    }

    #[test]
    fn format_bad_empty_message() {
        assert_eq!(format_bad("A", ""), "A BAD \r\n");
    }

    #[test]
    fn format_bye_empty_message() {
        assert_eq!(format_bye(""), "* BYE \r\n");
    }

    #[test]
    fn all_tagged_responses_end_with_crlf() {
        assert!(format_ok("t", "msg").ends_with("\r\n"));
        assert!(format_no("t", "msg").ends_with("\r\n"));
        assert!(format_bad("t", "msg").ends_with("\r\n"));
    }

    #[test]
    fn all_untagged_responses_start_with_star() {
        assert!(format_capability(&["IMAP4rev1"]).starts_with("* "));
        assert!(format_list("", "/", "X").starts_with("* "));
        assert!(format_fetch(1, &[]).starts_with("* "));
        assert!(format_bye("x").starts_with("* "));
        assert!(format_flags(&[]).starts_with("* "));
        assert!(format_exists(0).starts_with("* "));
        assert!(format_recent(0).starts_with("* "));
        assert!(format_quota("q", 0, 0).starts_with("* "));
        assert!(format_quotaroot("m", "q").starts_with("* "));
    }

    // --- additional edge-case tests ---

    #[test]
    fn format_ok_special_chars_in_tag() {
        assert_eq!(
            format_ok("A.1+2", "done"),
            "A.1+2 OK done\r\n"
        );
    }

    #[test]
    fn format_fetch_large_sequence_number() {
        let items = vec![("UID".to_string(), "999999".to_string())];
        assert_eq!(
            format_fetch(u32::MAX, &items),
            format!("* {} FETCH (UID 999999)\r\n", u32::MAX)
        );
    }

    #[test]
    fn format_capability_many_items() {
        let caps = vec![
            "IMAP4rev1", "IDLE", "NAMESPACE", "QUOTA", "CHILDREN",
            "UIDPLUS", "MOVE", "AUTH=PLAIN", "AUTH=LOGIN",
        ];
        let result = format_capability(&caps);
        assert!(result.starts_with("* CAPABILITY "));
        assert!(result.ends_with("\r\n"));
        // verify all capabilities present
        for cap in &caps {
            assert!(result.contains(cap));
        }
    }

    #[test]
    fn format_list_dot_delimiter() {
        let result = format_list("\\HasChildren", ".", "INBOX.Drafts");
        assert_eq!(
            result,
            "* LIST (\\HasChildren) \".\" INBOX.Drafts\r\n"
        );
    }

    #[test]
    fn format_flags_single_flag() {
        assert_eq!(format_flags(&["\\Seen"]), "* FLAGS (\\Seen)\r\n");
    }

    #[test]
    fn format_flags_system_and_custom() {
        let flags = &["\\Seen", "\\Flagged", "$Important", "$Junk"];
        let result = format_flags(flags);
        assert_eq!(
            result,
            "* FLAGS (\\Seen \\Flagged $Important $Junk)\r\n"
        );
    }

    #[test]
    fn format_bye_with_reason() {
        assert_eq!(
            format_bye("Autologout; idle for too long"),
            "* BYE Autologout; idle for too long\r\n"
        );
    }

    #[test]
    fn format_quota_at_limit() {
        assert_eq!(
            format_quota("user.bob", 10240, 10240),
            "* QUOTA \"user.bob\" (STORAGE 10240 10240)\r\n"
        );
    }

    #[test]
    fn all_untagged_responses_end_with_crlf() {
        assert!(format_capability(&[]).ends_with("\r\n"));
        assert!(format_list("", "/", "X").ends_with("\r\n"));
        assert!(format_fetch(1, &[]).ends_with("\r\n"));
        assert!(format_bye("x").ends_with("\r\n"));
        assert!(format_flags(&[]).ends_with("\r\n"));
        assert!(format_exists(0).ends_with("\r\n"));
        assert!(format_recent(0).ends_with("\r\n"));
        assert!(format_quota("q", 0, 0).ends_with("\r\n"));
        assert!(format_quotaroot("m", "q").ends_with("\r\n"));
    }
}
