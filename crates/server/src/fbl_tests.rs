//! Tests for `fbl` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

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
