//! feedback loop (FBL) processing
//!
//! parses ARF (Abuse Reporting Format, RFC 5965) complaint reports
//! and extracts the original recipient for suppression.

/// extract the original recipient from an ARF feedback report message
/// returns (original_recipient, feedback_type) if parseable
pub fn parse_arf_report(message: &[u8]) -> Option<(String, String)> {
    let text = String::from_utf8_lossy(message);

    // ARF reports contain "Content-Type: message/feedback-report"
    if !text.contains("feedback-report") {
        return None;
    }

    let mut original_rcpt = None;
    let mut feedback_type = "abuse".to_string();

    for line in text.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("Original-Rcpt-To:") {
            original_rcpt = Some(val.trim().to_lowercase());
        } else if let Some(val) = line.strip_prefix("Original-Mail-From:") {
            if original_rcpt.is_none() {
                original_rcpt = Some(val.trim().to_lowercase());
            }
        } else if let Some(val) = line.strip_prefix("Feedback-Type:") {
            feedback_type = val.trim().to_lowercase();
        }
    }

    original_rcpt.map(|rcpt| {
        let rcpt = rcpt.trim_start_matches('<').trim_end_matches('>').to_string();
        (rcpt, feedback_type)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_arf() {
        let msg = b"Content-Type: multipart/report; report-type=feedback-report\r\n\r\n\
            --boundary\r\nContent-Type: message/feedback-report\r\n\r\n\
            Feedback-Type: abuse\r\nOriginal-Rcpt-To: user@example.com\r\n";
        let result = parse_arf_report(msg);
        assert_eq!(result, Some(("user@example.com".into(), "abuse".into())));
    }

    #[test]
    fn parse_not_arf() {
        let msg = b"Subject: Hello\r\n\r\nJust a normal email";
        assert!(parse_arf_report(msg).is_none());
    }

    #[test]
    fn parse_angle_brackets() {
        let msg = b"feedback-report\r\nOriginal-Rcpt-To: <user@test.com>\r\n";
        let (rcpt, _) = parse_arf_report(msg).unwrap();
        assert_eq!(rcpt, "user@test.com");
    }

    #[test]
    fn parse_mail_from_fallback() {
        let msg = b"feedback-report\r\nFeedback-Type: complaint\r\nOriginal-Mail-From: sender@x.com\r\n";
        let (rcpt, ft) = parse_arf_report(msg).unwrap();
        assert_eq!(rcpt, "sender@x.com");
        assert_eq!(ft, "complaint");
    }
}
