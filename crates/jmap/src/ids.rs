//! JMAP id parsers.
//!
//! Visible ids are stable strings; the store uses `i64` primary keys
//! internally. JMAP doesn't constrain the id format, but `msg-` / `mb-`
//! prefixes make logs unambiguous and let other endpoints share the namespace.

/// Parse a JMAP email id of the form `msg-{i64}`.
pub fn parse_email_db_id(email_id: &str) -> Option<i64> {
    email_id.strip_prefix("msg-").and_then(|s| s.parse().ok())
}

/// Parse a JMAP mailbox id of the form `mb-{i64}`.
pub fn parse_mailbox_db_id(mb_id: &str) -> Option<i64> {
    mb_id.strip_prefix("mb-").and_then(|s| s.parse().ok())
}

/// Format an email id for the wire.
pub fn format_email_id(id: i64) -> String {
    format!("msg-{id}")
}

/// Format a mailbox id for the wire.
pub fn format_mailbox_id(id: i64) -> String {
    format!("mb-{id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_email_ok() {
        assert_eq!(parse_email_db_id("msg-42"), Some(42));
        assert_eq!(parse_email_db_id("msg-0"), Some(0));
    }

    #[test]
    fn parse_email_rejects_wrong_prefix() {
        assert_eq!(parse_email_db_id("42"), None);
        assert_eq!(parse_email_db_id("mb-42"), None);
        assert_eq!(parse_email_db_id(""), None);
    }

    #[test]
    fn parse_email_rejects_non_numeric() {
        assert_eq!(parse_email_db_id("msg-abc"), None);
        assert_eq!(parse_email_db_id("msg-"), None);
    }

    #[test]
    fn parse_mailbox_ok() {
        assert_eq!(parse_mailbox_db_id("mb-7"), Some(7));
    }

    #[test]
    fn format_round_trip() {
        assert_eq!(parse_email_db_id(&format_email_id(99)), Some(99));
        assert_eq!(parse_mailbox_db_id(&format_mailbox_id(99)), Some(99));
    }
}
