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
    write!(buf, "From: Mail Delivery System <mailer-daemon@{reporting_mta}>\r\n").unwrap();
    write!(buf, "To: <{sender}>\r\n").unwrap();
    write!(buf, "Subject: Delivery Status Notification (Failure)\r\n").unwrap();
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
                assert!(i > 0 && dsn.as_bytes()[i - 1] == b'\r', "bare \\n at byte {i}");
            }
        }
    }
}
