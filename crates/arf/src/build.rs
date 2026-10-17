//! Writing a feedback report: the inverse of [`crate::parse`].

/// What a generated report says about one complained-about message.
#[derive(Debug, Clone)]
pub struct ReportSpec<'a> {
    /// `From:` of the report itself.
    pub from: &'a str,
    /// Who the report is addressed to, usually the original sender.
    pub to: &'a str,
    /// `Feedback-Type`, e.g. `abuse`.
    pub feedback_type: &'a str,
    /// `User-Agent` of the reporting software.
    pub user_agent: &'a str,
    /// `Original-Mail-From`: envelope sender of the complained-about message.
    pub original_mail_from: &'a str,
    /// `Original-Rcpt-To`: the recipient who complained.
    pub original_rcpt_to: &'a str,
    /// `Arrival-Date`, RFC 5322 date.
    pub arrival_date: &'a str,
    /// `Message-ID` of the report, angle brackets included.
    pub message_id: &'a str,
    /// The complained-about message; its header block is quoted.
    pub original: &'a [u8],
}

/// An RFC 5965 `multipart/report; report-type=feedback-report` message:
/// a human-readable part, the `message/feedback-report` part, and the
/// original message's headers as `text/rfc822-headers`.
///
/// ```
/// use mailrs_arf::{ReportSpec, build, parse};
/// let raw = build(&ReportSpec {
///     from: "fbl@isp.example",
///     to: "sender@shop.example",
///     feedback_type: "abuse",
///     user_agent: "example/1.0",
///     original_mail_from: "sender@shop.example",
///     original_rcpt_to: "user@isp.example",
///     arrival_date: "Fri, 16 Oct 2026 10:00:00 +0000",
///     message_id: "<r1@isp.example>",
///     original: b"Subject: sale\r\n\r\nbuy",
/// });
/// let report = parse(&raw).unwrap();
/// assert_eq!(report.complainant(), Some("user@isp.example"));
/// ```
#[must_use]
pub fn build(spec: &ReportSpec<'_>) -> Vec<u8> {
    let boundary = format!("=_arf_{}", boundary_seed(spec.message_id));
    let line = |s: &str| s.replace(['\r', '\n'], " ");
    let mut out = format!(
        "From: {from}\r\nTo: {to}\r\nSubject: Abuse report\r\nDate: {date}\r\n\
         Message-ID: {mid}\r\nAuto-Submitted: auto-generated\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/report; report-type=feedback-report; boundary=\"{boundary}\"\r\n\r\n\
         --{boundary}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n\
         This is a feedback report about a message sent to {rcpt}.\r\n\r\n\
         --{boundary}\r\nContent-Type: message/feedback-report\r\n\r\n\
         Feedback-Type: {kind}\r\nUser-Agent: {agent}\r\nVersion: 1\r\n\
         Original-Mail-From: <{mail_from}>\r\nOriginal-Rcpt-To: <{rcpt}>\r\n\
         Arrival-Date: {date}\r\n\r\n\
         --{boundary}\r\nContent-Type: text/rfc822-headers\r\n\r\n",
        from = line(spec.from),
        to = line(spec.to),
        date = line(spec.arrival_date),
        mid = line(spec.message_id),
        rcpt = line(spec.original_rcpt_to),
        kind = line(spec.feedback_type),
        agent = line(spec.user_agent),
        mail_from = line(spec.original_mail_from),
    )
    .into_bytes();
    out.extend_from_slice(header_block(spec.original));
    out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    out
}

fn header_block(message: &[u8]) -> &[u8] {
    let end = memchr::memmem::find(message, b"\r\n\r\n")
        .map(|i| i + 2)
        .or_else(|| memchr::memmem::find(message, b"\n\n").map(|i| i + 1))
        .unwrap_or(message.len());
    &message[..end]
}

// derived from the report's id, so building the same report twice gives the same bytes
fn boundary_seed(message_id: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in message_id.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_built_report_parses_back_and_quotes_only_the_headers() {
        let raw = build(&ReportSpec {
            from: "fbl@isp.example",
            to: "sender@shop.example",
            feedback_type: "abuse",
            user_agent: "test/1",
            original_mail_from: "sender@shop.example",
            original_rcpt_to: "User@ISP.example",
            arrival_date: "Fri, 16 Oct 2026 10:00:00 +0000",
            message_id: "<r1@isp.example>",
            original: b"Subject: sale\r\nFrom: sender@shop.example\r\n\r\nsecret body",
        });
        let report = crate::parse(&raw).unwrap();
        assert_eq!(report.feedback_type, "abuse");
        assert_eq!(report.original_rcpt_to.as_deref(), Some("user@isp.example"));
        assert_eq!(
            report.original_mail_from.as_deref(),
            Some("sender@shop.example")
        );
        let text = String::from_utf8(raw).unwrap();
        assert!(text.contains("Subject: sale"));
        assert!(!text.contains("secret body"));
    }

    #[test]
    fn a_header_value_cannot_add_a_line() {
        let raw = build(&ReportSpec {
            from: "a@b.example",
            to: "c@d.example\r\nBcc: x@y.example",
            feedback_type: "abuse",
            user_agent: "t",
            original_mail_from: "c@d.example",
            original_rcpt_to: "u@b.example",
            arrival_date: "d",
            message_id: "<m@b.example>",
            original: b"",
        });
        assert!(!String::from_utf8(raw).unwrap().contains("\r\nBcc:"));
    }
}
