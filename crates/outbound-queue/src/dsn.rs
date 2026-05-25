use std::fmt::Write;

/// format a DSN (Delivery Status Notification) bounce message per RFC 3464
/// uses CRLF line endings per RFC 5322
pub fn format_dsn(
    reporting_mta: &str,
    sender: &str,
    recipient: &str,
    error: &str,
    message_id: Option<&str>,
) -> String {
    let mut buf = String::new();

    // headers
    let now = chrono::Utc::now();
    let date = now.format("%a, %d %b %Y %H:%M:%S +0000");
    let dsn_id = format!(
        "{}.{}@{}",
        now.timestamp(),
        std::process::id(),
        reporting_mta
    );

    write!(
        buf,
        "From: Mail Delivery System <mailer-daemon@{reporting_mta}>\r\n"
    )
    .unwrap();
    write!(buf, "To: <{sender}>\r\n").unwrap();
    write!(buf, "Date: {date}\r\n").unwrap();
    write!(buf, "Message-ID: <{dsn_id}>\r\n").unwrap();
    write!(buf, "Subject: Delivery Status Notification (Failure)\r\n").unwrap();
    write!(buf, "Auto-Submitted: auto-replied\r\n").unwrap();
    write!(buf, "MIME-Version: 1.0\r\n").unwrap();
    write!(
        buf,
        "Content-Type: multipart/report; report-type=delivery-status; boundary=\"dsn-boundary\"\r\n"
    )
    .unwrap();
    if let Some(mid) = message_id {
        write!(buf, "References: <{mid}>\r\n").unwrap();
    }
    write!(buf, "\r\n").unwrap();

    // human-readable part
    write!(buf, "--dsn-boundary\r\n").unwrap();
    write!(buf, "Content-Type: text/plain; charset=utf-8\r\n").unwrap();
    write!(buf, "\r\n").unwrap();
    write!(
        buf,
        "Your message to <{recipient}> could not be delivered.\r\n"
    )
    .unwrap();
    write!(buf, "\r\n").unwrap();
    write!(buf, "Error: {error}\r\n").unwrap();
    write!(buf, "\r\n").unwrap();

    // machine-readable part
    write!(buf, "--dsn-boundary\r\n").unwrap();
    write!(buf, "Content-Type: message/delivery-status\r\n").unwrap();
    write!(buf, "\r\n").unwrap();
    write!(buf, "Reporting-MTA: dns; {reporting_mta}\r\n").unwrap();
    write!(buf, "\r\n").unwrap();
    write!(buf, "Final-Recipient: rfc822; {recipient}\r\n").unwrap();
    write!(buf, "Action: failed\r\n").unwrap();
    write!(buf, "Status: 5.0.0\r\n").unwrap();
    write!(buf, "Diagnostic-Code: smtp; {error}\r\n").unwrap();
    write!(buf, "\r\n").unwrap();

    write!(buf, "--dsn-boundary--\r\n").unwrap();
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_basic() {
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "550 User not found",
            None,
        );
        assert!(dsn.contains("From: Mail Delivery System <mailer-daemon@mx.example.com>\r\n"));
        assert!(dsn.contains("To: <sender@example.com>\r\n"));
        assert!(dsn.contains("Final-Recipient: rfc822; rcpt@remote.com\r\n"));
        assert!(dsn.contains("Diagnostic-Code: smtp; 550 User not found\r\n"));
        assert!(!dsn.contains("References:"));
    }

    #[test]
    fn format_with_message_id() {
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "421 try again",
            Some("abc123@example.com"),
        );
        assert!(dsn.contains("References: <abc123@example.com>\r\n"));
    }

    #[test]
    fn dsn_structure() {
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "550 no such user",
            None,
        );
        assert!(dsn.contains("multipart/report"));
        assert!(dsn.contains("--dsn-boundary\r\n"));
        assert!(dsn.contains("--dsn-boundary--\r\n"));
        assert!(dsn.contains("message/delivery-status"));
        assert!(dsn.contains("Reporting-MTA: dns; mx.example.com\r\n"));
        assert!(dsn.contains("Action: failed\r\n"));
        assert!(dsn.contains("Status: 5.0.0\r\n"));
    }

    #[test]
    fn special_chars_in_error() {
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "550 5.1.1 <rcpt@remote.com>: Recipient address rejected",
            None,
        );
        assert!(dsn.contains("550 5.1.1 <rcpt@remote.com>: Recipient address rejected"));
    }

    #[test]
    fn uses_crlf_line_endings() {
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "550 error",
            None,
        );
        // no bare \n (every \n should be preceded by \r)
        for (i, &b) in dsn.as_bytes().iter().enumerate() {
            if b == b'\n' {
                assert!(
                    i > 0 && dsn.as_bytes()[i - 1] == b'\r',
                    "bare \\n at byte {i}"
                );
            }
        }
    }

    #[test]
    fn dsn_human_readable_part_mentions_recipient() {
        let dsn = format_dsn(
            "mx.example.com",
            "alice@example.com",
            "bob@remote.org",
            "550 no such user",
            None,
        );
        assert!(dsn.contains("<bob@remote.org> could not be delivered"));
    }

    #[test]
    fn dsn_boundary_appears_exactly_three_times() {
        // open boundary x2 + closing boundary x1 = 3
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "550 error",
            None,
        );
        let open_count = dsn.matches("--dsn-boundary\r\n").count();
        let close_count = dsn.matches("--dsn-boundary--\r\n").count();
        assert_eq!(open_count, 2, "expected 2 open boundaries");
        assert_eq!(close_count, 1, "expected 1 closing boundary");
    }

    #[test]
    fn dsn_no_references_when_no_message_id() {
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "550 error",
            None,
        );
        assert!(!dsn.contains("References:"));
    }

    #[test]
    fn dsn_error_appears_in_diagnostic_code() {
        let err = "421 Service temporarily unavailable";
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            err,
            None,
        );
        assert!(dsn.contains(&format!("Diagnostic-Code: smtp; {err}")));
    }

    #[test]
    fn dsn_reporting_mta_field() {
        let dsn = format_dsn(
            "relay.myhost.net",
            "sender@example.com",
            "rcpt@remote.com",
            "550 error",
            None,
        );
        assert!(dsn.contains("Reporting-MTA: dns; relay.myhost.net\r\n"));
    }

    #[test]
    fn dsn_action_and_status_fixed() {
        // DSN for permanent failure must always use Action: failed and Status: 5.0.0
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "any error",
            None,
        );
        assert!(dsn.contains("Action: failed\r\n"));
        assert!(dsn.contains("Status: 5.0.0\r\n"));
    }

    #[test]
    fn dsn_from_header_uses_mailer_daemon() {
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "error",
            None,
        );
        assert!(dsn.starts_with("From: Mail Delivery System <mailer-daemon@mx.example.com>\r\n"));
    }

    #[test]
    fn dsn_subject_is_failure() {
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "error",
            None,
        );
        assert!(dsn.contains("Subject: Delivery Status Notification (Failure)\r\n"));
    }

    #[test]
    fn dsn_mime_version_present() {
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "error",
            None,
        );
        assert!(dsn.contains("MIME-Version: 1.0\r\n"));
    }

    #[test]
    fn dsn_auto_submitted_header() {
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "error",
            None,
        );
        assert!(dsn.contains("Auto-Submitted: auto-replied\r\n"));
    }

    #[test]
    fn dsn_date_header_present() {
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "error",
            None,
        );
        assert!(dsn.contains("Date: "));
        // date header should contain +0000 (UTC)
        assert!(dsn.contains("+0000\r\n"));
    }

    #[test]
    fn dsn_message_id_contains_reporting_mta() {
        let dsn = format_dsn(
            "relay.myhost.net",
            "sender@example.com",
            "rcpt@remote.com",
            "error",
            None,
        );
        // message-id should contain the reporting MTA hostname
        assert!(dsn.contains("Message-ID: <"));
        assert!(dsn.contains("@relay.myhost.net>\r\n"));
    }

    #[test]
    fn dsn_content_type_multipart_report() {
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "error",
            None,
        );
        assert!(dsn.contains(
            "Content-Type: multipart/report; report-type=delivery-status; boundary=\"dsn-boundary\"\r\n"
        ));
    }

    #[test]
    fn dsn_text_plain_part_present() {
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "error",
            None,
        );
        assert!(dsn.contains("Content-Type: text/plain; charset=utf-8\r\n"));
    }

    #[test]
    fn dsn_delivery_status_part_present() {
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "error",
            None,
        );
        assert!(dsn.contains("Content-Type: message/delivery-status\r\n"));
    }

    #[test]
    fn dsn_final_recipient_rfc822_format() {
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "user@example.org",
            "error",
            None,
        );
        assert!(dsn.contains("Final-Recipient: rfc822; user@example.org\r\n"));
    }

    #[test]
    fn dsn_empty_error_string() {
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "",
            None,
        );
        assert!(dsn.contains("Error: \r\n"));
        assert!(dsn.contains("Diagnostic-Code: smtp; \r\n"));
    }

    #[test]
    fn dsn_long_error_message() {
        let long_error = "5".repeat(1000);
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            &long_error,
            None,
        );
        assert!(dsn.contains(&long_error));
    }

    #[test]
    fn dsn_various_smtp_error_codes() {
        let errors = [
            "421 Service not available",
            "450 Requested mail action not taken: mailbox unavailable",
            "451 Requested action aborted: local error in processing",
            "452 Requested action not taken: insufficient system storage",
            "550 Requested action not taken: mailbox unavailable",
            "551 User not local",
            "552 Requested mail action aborted: exceeded storage allocation",
            "553 Requested action not taken: mailbox name not allowed",
            "554 Transaction failed",
        ];
        for err in &errors {
            let dsn = format_dsn(
                "mx.example.com",
                "sender@example.com",
                "rcpt@remote.com",
                err,
                None,
            );
            assert!(
                dsn.contains(&format!("Diagnostic-Code: smtp; {err}\r\n")),
                "error code not found in DSN: {err}"
            );
        }
    }

    #[test]
    fn dsn_header_body_separation() {
        // RFC 5322: headers and body separated by empty line (CRLF CRLF)
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "error",
            None,
        );
        assert!(
            dsn.contains("\r\n\r\n"),
            "DSN must have header/body separator"
        );
    }

    #[test]
    fn dsn_references_header_angle_brackets() {
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "error",
            Some("test-id@example.com"),
        );
        assert!(dsn.contains("References: <test-id@example.com>\r\n"));
    }

    #[test]
    fn dsn_different_reporting_mta_values() {
        let mtas = ["mx1.example.com", "relay.internal.corp", "mail.golia.jp"];
        for mta in &mtas {
            let dsn = format_dsn(mta, "sender@test.com", "rcpt@test.com", "err", None);
            assert!(dsn.contains(&format!(
                "From: Mail Delivery System <mailer-daemon@{mta}>\r\n"
            )));
            assert!(dsn.contains(&format!("Reporting-MTA: dns; {mta}\r\n")));
            assert!(dsn.contains(&format!("@{mta}>\r\n")));
        }
    }

    #[test]
    fn dsn_to_header_uses_angle_brackets() {
        let dsn = format_dsn(
            "mx.example.com",
            "alice@example.com",
            "bob@remote.com",
            "error",
            None,
        );
        assert!(dsn.contains("To: <alice@example.com>\r\n"));
    }

    #[test]
    fn dsn_machine_readable_part_has_blank_line_between_mta_and_recipient() {
        // per RFC 3464, per-message fields and per-recipient fields are separated by blank line
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "error",
            None,
        );
        assert!(dsn.contains("Reporting-MTA: dns; mx.example.com\r\n\r\nFinal-Recipient:"));
    }

    #[test]
    fn dsn_output_is_nonempty() {
        let dsn = format_dsn("h", "s@s", "r@r", "e", None);
        assert!(!dsn.is_empty());
    }

    #[test]
    fn dsn_with_message_id_containing_special_chars() {
        let msg_id = "abc+def/ghi=jkl@example.com";
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "error",
            Some(msg_id),
        );
        assert!(dsn.contains(&format!("References: <{msg_id}>\r\n")));
    }

    #[test]
    fn dsn_human_readable_error_present() {
        let dsn = format_dsn(
            "mx.example.com",
            "sender@example.com",
            "rcpt@remote.com",
            "550 mailbox full",
            None,
        );
        // human-readable part should contain the error
        assert!(dsn.contains("Error: 550 mailbox full\r\n"));
    }
}
