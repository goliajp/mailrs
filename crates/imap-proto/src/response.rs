/// format CAPABILITY response
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
}
