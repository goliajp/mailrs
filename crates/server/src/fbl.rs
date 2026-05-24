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
#[path = "fbl_tests.rs"]
mod tests;
