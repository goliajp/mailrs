//! RFC 3464 Delivery Status Notification (DSN) builder.
//!
//! Wraps `mailrs-mail-builder`'s canonical `MessageBuilder` so the
//! multipart/report envelope, header folding, encoded-word
//! handling, and boundary collision-scan are all shared with the
//! rest of the outbound stack. The DSN-specific `message/delivery-
//! status` machine-readable part is constructed inline (RFC 3464
//! §2 grammar — pure ASCII key/value pairs).

use mailrs_mail_builder::{Attachment, MessageBuilder};

/// Format a DSN (Delivery Status Notification) bounce message per
/// RFC 3464. The returned message uses CRLF line endings (RFC 5322
/// §2.1) and `multipart/report; report-type=delivery-status`.
pub fn format_dsn(
    reporting_mta: &str,
    sender: &str,
    recipient: &str,
    error: &str,
    message_id: Option<&str>,
) -> String {
    let now = chrono::Utc::now();
    let dsn_id = format!(
        "<{}.{}@{}>",
        now.timestamp(),
        std::process::id(),
        reporting_mta,
    );

    // Machine-readable RFC 3464 §2 fields. Pure ASCII; we want
    // these bytes to land in the part body unchanged, so a 7bit
    // CTE will pick them up automatically.
    let machine_body = format!(
        "Reporting-MTA: dns; {reporting_mta}\r\n\
         \r\n\
         Final-Recipient: rfc822; {recipient}\r\n\
         Action: failed\r\n\
         Status: 5.0.0\r\n\
         Diagnostic-Code: smtp; {error}\r\n",
    );

    // Human-readable part body — RFC 3464 §2 calls for at least
    // one preceding part of any type that explains the failure to
    // the recipient (the bounced-back original sender).
    let human = format!(
        "Your message to <{recipient}> could not be delivered.\r\n\
         \r\n\
         Error: {error}\r\n",
    );

    let mut b = MessageBuilder::new()
        .from(format!(
            "Mail Delivery System <mailer-daemon@{reporting_mta}>"
        ))
        .to(sender)
        .subject("Delivery Status Notification (Failure)")
        .header("Auto-Submitted", "auto-replied")
        .text_body(human)
        .attachment(Attachment::new(
            "delivery-status.txt",
            "message/delivery-status",
            machine_body.into_bytes(),
        ))
        .report_type("delivery-status")
        .message_id(dsn_id);

    if let Some(mid) = message_id {
        b = b.header("References", format!("<{mid}>"));
    }

    let bytes = b.build();
    String::from_utf8(bytes).expect("mail-builder output is ASCII-safe by construction")
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
        // mail-builder renders bare addresses without angle brackets;
        // this is RFC 5322 §3.4 addr-spec form, equally valid as
        // name-addr <addr-spec>.
        assert!(dsn.contains("To: sender@example.com\r\n"));
        // unfold before matching so soft-fold doesn't trip the substring check
        let unfold = dsn.replace("\r\n ", " ").replace("\r\n\t", " ");
        assert!(unfold.contains("Final-Recipient: rfc822; rcpt@remote.com"));
        assert!(unfold.contains("Diagnostic-Code: smtp; 550 User not found"));
        assert!(!unfold.contains("References:"));
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
        assert!(dsn.contains("References: <abc123@example.com>"));
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
        // unfold for substring checks — header values may wrap
        let unfold = dsn.replace("\r\n ", " ").replace("\r\n\t", " ");
        assert!(unfold.contains("multipart/report"));
        assert!(unfold.contains("report-type=delivery-status"));
        assert!(unfold.contains("message/delivery-status"));
        // body still ends with a closing boundary marker — exact
        // boundary string is random per call (mail-builder), so
        // we just check there's at least one `--` envelope line
        let envelope_opens = dsn.matches("\r\n--").count();
        assert!(envelope_opens >= 2, "got {envelope_opens} envelope markers");
        assert!(unfold.contains("Reporting-MTA: dns; mx.example.com"));
        assert!(unfold.contains("Action: failed"));
        assert!(unfold.contains("Status: 5.0.0"));
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
        let unfold = dsn.replace("\r\n ", " ").replace("\r\n\t", " ");
        assert!(
            unfold.contains("550 5.1.1 <rcpt@remote.com>: Recipient address rejected"),
            "diagnostic-code line missing in: {unfold}",
        );
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
            "sender@example.com",
            "rcpt@remote.com",
            "550 error",
            None,
        );
        // body of the first part should mention the recipient address
        let unfold = dsn.replace("\r\n ", " ").replace("\r\n\t", " ");
        assert!(unfold.contains("<rcpt@remote.com>"));
    }
}
