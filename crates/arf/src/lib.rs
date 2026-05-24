#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Internal layout: [`parse`] does the full one-shot extraction.
//! Helpers ([`is_arf`], header continuation handling) are private —
//! the public surface is intentionally a single function returning
//! a single struct.

/// Parsed RFC 5965 ARF feedback report.
///
/// All fields are owned `String` (not `&str`) — callers typically
/// persist these into a suppression-list DB or fire a webhook, so
/// borrow-from-input would be hostile to the typical use case. Cost
/// is one short allocation per non-empty field.
///
/// Fields default to `None` (or `"abuse"` for `feedback_type`) when
/// the report omits them. Per RFC 5965 §3.2 the only `MUST`-present
/// field on the `message/feedback-report` part is `Feedback-Type`,
/// `User-Agent`, and `Version` — everything else is `SHOULD` or
/// optional, so be ready for `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Feedback-Type (§3.1) — `abuse`, `fraud`, `not-spam`,
    /// `virus`, `other`, etc. Lowercase normalized. Defaults to
    /// `"abuse"` if the header is absent (some FBL providers omit
    /// it for the most common case).
    pub feedback_type: String,
    /// User-Agent (§3.1) — software / version that produced this
    /// report.
    pub user_agent: Option<String>,
    /// Version (§3.1) — ARF version. RFC 5965 uses `"1"`.
    pub version: Option<String>,
    /// Original-Mail-From (§3.2) — envelope From of the complained-
    /// about message. Angle brackets stripped. Lowercase normalized.
    pub original_mail_from: Option<String>,
    /// Original-Rcpt-To (§3.2) — envelope recipient who complained.
    /// Angle brackets stripped. Lowercase normalized.
    pub original_rcpt_to: Option<String>,
    /// Arrival-Date (§3.2) — when the complained-about message
    /// arrived at the FBL provider.
    pub arrival_date: Option<String>,
    /// Source-IP (§3.2) — the IP that delivered the complained-
    /// about message to the FBL provider.
    pub source_ip: Option<String>,
    /// Reported-Domain (§3.2) — domain of the sender as the FBL
    /// provider sees it. Lowercase normalized.
    pub reported_domain: Option<String>,
    /// Reported-URI (§3.2) — URI inside the complained-about
    /// message body (typically `mailto:` or `http(s):` link). If
    /// the report has multiple `Reported-URI:` headers, only the
    /// first is kept; downstream callers needing all of them should
    /// parse the raw multipart themselves.
    pub reported_uri: Option<String>,
    /// Authentication-Results (§3.2) — RFC 8601-formatted auth
    /// verdict captured by the FBL provider at receipt time.
    pub authentication_results: Option<String>,
    /// Incidents (§3.2) — when the FBL provider aggregates multiple
    /// reports, this counts how many distinct complaints this one
    /// report represents.
    pub incidents: Option<String>,
}

impl Report {
    /// Convenience: the most actionable single fact in any ARF
    /// report — who complained. Falls back to `original_mail_from`
    /// when the report omits `Original-Rcpt-To` (older FBL formats
    /// invert which one is present).
    ///
    /// Returns `None` if neither header is present, which means the
    /// report is structurally invalid and should be logged + dropped.
    #[must_use]
    pub fn complainant(&self) -> Option<&str> {
        self.original_rcpt_to
            .as_deref()
            .or(self.original_mail_from.as_deref())
    }
}

/// Parse an RFC 5965 ARF feedback report from a full
/// `multipart/report` message body.
///
/// Returns `None` if the input does not contain a
/// `message/feedback-report` part marker (i.e. the input is not an
/// ARF report at all). This is the fast cold-path early exit; the
/// per-byte cost of the substring search is bounded by the message
/// size.
///
/// On a positive identification, returns `Some(Report)` populated
/// with every field that was present in the `message/feedback-report`
/// part. Unknown / unparseable fields are silently skipped — the
/// parser is tolerant of vendor-specific extensions and the
/// pre-standard 0.x ARF format.
///
/// Does **not** attempt to validate the surrounding MIME boundary or
/// verify the third (`message/rfc822`) part. If you need full MIME
/// integrity, parse the input through `mailrs-mime` first and pass
/// the `message/feedback-report` subpart's body to this function.
///
/// ```
/// use mailrs_arf::parse;
/// let report = parse(b"feedback-report\r\nFeedback-Type: abuse\r\n\
///     Original-Rcpt-To: u@example.com\r\n");
/// let r = report.unwrap();
/// assert_eq!(r.feedback_type, "abuse");
/// assert_eq!(r.original_rcpt_to.as_deref(), Some("u@example.com"));
/// ```
#[must_use]
pub fn parse(message: &[u8]) -> Option<Report> {
    if !is_arf(message) {
        return None;
    }

    let mut feedback_type = "abuse".to_string();
    let mut user_agent: Option<String> = None;
    let mut version: Option<String> = None;
    let mut original_mail_from: Option<String> = None;
    let mut original_rcpt_to: Option<String> = None;
    let mut arrival_date: Option<String> = None;
    let mut source_ip: Option<String> = None;
    let mut reported_domain: Option<String> = None;
    let mut reported_uri: Option<String> = None;
    let mut authentication_results: Option<String> = None;
    let mut incidents: Option<String> = None;

    for (key, value) in headers(message) {
        let lower = key.to_ascii_lowercase();
        match lower.as_str() {
            "feedback-type" => feedback_type = value.to_ascii_lowercase(),
            "user-agent" => user_agent = Some(value),
            "version" => version = Some(value),
            "original-mail-from" if original_mail_from.is_none() => {
                original_mail_from = Some(strip_addr(&value));
            }
            "original-rcpt-to" if original_rcpt_to.is_none() => {
                original_rcpt_to = Some(strip_addr(&value));
            }
            "arrival-date" if arrival_date.is_none() => arrival_date = Some(value),
            "source-ip" if source_ip.is_none() => source_ip = Some(value),
            "reported-domain" if reported_domain.is_none() => {
                reported_domain = Some(value.to_ascii_lowercase());
            }
            "reported-uri" if reported_uri.is_none() => reported_uri = Some(value),
            "authentication-results" if authentication_results.is_none() => {
                authentication_results = Some(value);
            }
            "incidents" if incidents.is_none() => incidents = Some(value),
            _ => {}
        }
    }

    Some(Report {
        feedback_type,
        user_agent,
        version,
        original_mail_from,
        original_rcpt_to,
        arrival_date,
        source_ip,
        reported_domain,
        reported_uri,
        authentication_results,
        incidents,
    })
}

/// Fast cold-path: does this look like an ARF report at all?
/// Identification rests on the literal `feedback-report` substring,
/// which appears in every legitimate ARF report's
/// `Content-Type: message/feedback-report` header. A 24-byte
/// substring search across the whole input — fine for typical
/// FBL message sizes (< 16 KB).
fn is_arf(message: &[u8]) -> bool {
    const MARKER: &[u8] = b"feedback-report";
    message.windows(MARKER.len()).any(|w| w == MARKER)
}

/// Strip RFC 5322 `<addr>` angle brackets + lowercase. Used for
/// the two envelope-address fields (`Original-Mail-From`,
/// `Original-Rcpt-To`) so downstream suppression-list lookups can
/// use the value as a key directly.
fn strip_addr(value: &str) -> String {
    value
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_ascii_lowercase()
}

/// Walk every header-shaped line in the input, across MIME part
/// boundaries. ARF reports are nested inside a `multipart/report`
/// envelope — the feedback fields we care about live in the second
/// part's headers, after a blank line + boundary line + another set
/// of part headers. Rather than re-implement a full MIME parser,
/// we treat the whole input as a flat stream of headers: blank lines
/// and `--boundary` lines reset the per-header continuation state
/// but don't end iteration.
///
/// Continuation lines (RFC 5322 §2.2.3) starting with whitespace
/// fold into the preceding header. The first occurrence of each
/// header name wins (RFC 5965 §3.2 — each field appears at most once
/// per report).
fn headers(message: &[u8]) -> impl Iterator<Item = (&str, String)> + '_ {
    let text = std::str::from_utf8(message).unwrap_or("");
    HeaderIter {
        lines: text.lines(),
        pending: None,
    }
}

struct HeaderIter<'a> {
    lines: std::str::Lines<'a>,
    pending: Option<(&'a str, String)>,
}

impl<'a> Iterator for HeaderIter<'a> {
    type Item = (&'a str, String);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let line = match self.lines.next() {
                Some(l) => l,
                None => return self.pending.take(),
            };
            // Blank line or MIME boundary line: flush pending header
            // and reset (next non-blank line starts a fresh header
            // context).
            if line.trim().is_empty() || line.starts_with("--") {
                if let Some(out) = self.pending.take() {
                    return Some(out);
                }
                continue;
            }
            if line.starts_with(' ') || line.starts_with('\t') {
                // Continuation: append to pending value (separated
                // by a single space per RFC 5322 §2.2.3 unfolding).
                if let Some((_, ref mut value)) = self.pending {
                    value.push(' ');
                    value.push_str(line.trim());
                    continue;
                }
                // Stray continuation with no preceding header —
                // ignore (malformed input).
                continue;
            }
            // Start of a new header. Yield the pending one first,
            // then stash this line as the new pending.
            let new_pending = line.split_once(':').map(|(k, v)| (k, v.trim().to_string()));
            let out = self.pending.take();
            self.pending = new_pending;
            if out.is_some() {
                return out;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_arf() {
        let msg = b"Content-Type: multipart/report; report-type=feedback-report\r\n\
            \r\n\
            --boundary\r\n\
            Content-Type: message/feedback-report\r\n\
            \r\n\
            Feedback-Type: abuse\r\n\
            Original-Rcpt-To: user@example.com\r\n";
        let r = parse(msg).expect("ARF report");
        assert_eq!(r.feedback_type, "abuse");
        assert_eq!(r.original_rcpt_to.as_deref(), Some("user@example.com"));
    }

    #[test]
    fn parse_not_arf_returns_none() {
        let msg = b"Subject: Hello\r\n\r\nJust a normal email";
        assert!(parse(msg).is_none());
    }

    #[test]
    fn parse_strips_angle_brackets() {
        let msg = b"feedback-report\r\nOriginal-Rcpt-To: <user@test.com>\r\n";
        let r = parse(msg).unwrap();
        assert_eq!(r.original_rcpt_to.as_deref(), Some("user@test.com"));
    }

    #[test]
    fn parse_mail_from_fallback() {
        let msg = b"feedback-report\r\n\
            Feedback-Type: complaint\r\n\
            Original-Mail-From: sender@x.com\r\n";
        let r = parse(msg).unwrap();
        assert_eq!(r.feedback_type, "complaint");
        assert_eq!(r.complainant(), Some("sender@x.com"));
        assert_eq!(r.original_rcpt_to, None);
    }

    #[test]
    fn parse_hotmail_style_report() {
        let msg = b"From: staff@hotmail.com\r\n\
            Subject: complaint\r\n\
            Content-Type: multipart/report; report-type=feedback-report\r\n\
            \r\n\
            --b\r\n\
            Content-Type: message/feedback-report\r\n\
            \r\n\
            Feedback-Type: abuse\r\n\
            User-Agent: Hotmail FBL\r\n\
            Version: 1\r\n\
            Original-Mail-From: <bulk@example.com>\r\n\
            Original-Rcpt-To: <victim@hotmail.com>\r\n\
            Arrival-Date: Sun, 25 May 2026 10:00:00 +0000\r\n\
            Source-IP: 192.0.2.42\r\n\
            Reported-Domain: example.com\r\n\
            \r\n\
            --b--\r\n";
        let r = parse(msg).unwrap();
        assert_eq!(r.feedback_type, "abuse");
        assert_eq!(r.user_agent.as_deref(), Some("Hotmail FBL"));
        assert_eq!(r.version.as_deref(), Some("1"));
        assert_eq!(r.original_mail_from.as_deref(), Some("bulk@example.com"));
        assert_eq!(r.original_rcpt_to.as_deref(), Some("victim@hotmail.com"));
        assert_eq!(r.source_ip.as_deref(), Some("192.0.2.42"));
        assert_eq!(r.reported_domain.as_deref(), Some("example.com"));
    }

    #[test]
    fn parse_lowercase_normalizes_feedback_type() {
        let msg = b"feedback-report\r\nFeedback-Type: ABUSE\r\n";
        assert_eq!(parse(msg).unwrap().feedback_type, "abuse");
    }

    #[test]
    fn parse_lowercase_normalizes_addresses() {
        let msg = b"feedback-report\r\n\
            Original-Rcpt-To: <Victim@HotMail.COM>\r\n\
            Original-Mail-From: <Sender@Example.COM>\r\n";
        let r = parse(msg).unwrap();
        assert_eq!(r.original_rcpt_to.as_deref(), Some("victim@hotmail.com"));
        assert_eq!(r.original_mail_from.as_deref(), Some("sender@example.com"));
    }

    #[test]
    fn parse_lowercase_normalizes_reported_domain() {
        let msg = b"feedback-report\r\nReported-Domain: Example.COM\r\n";
        assert_eq!(
            parse(msg).unwrap().reported_domain.as_deref(),
            Some("example.com")
        );
    }

    #[test]
    fn parse_default_feedback_type_is_abuse_when_omitted() {
        let msg = b"feedback-report\r\nOriginal-Rcpt-To: u@e.com\r\n";
        assert_eq!(parse(msg).unwrap().feedback_type, "abuse");
    }

    #[test]
    fn parse_first_value_wins_for_duplicate_headers() {
        let msg = b"feedback-report\r\n\
            Original-Rcpt-To: first@e.com\r\n\
            Original-Rcpt-To: second@e.com\r\n";
        assert_eq!(
            parse(msg).unwrap().original_rcpt_to.as_deref(),
            Some("first@e.com")
        );
    }

    #[test]
    fn parse_header_continuation_lines() {
        let msg = b"feedback-report\r\n\
            Authentication-Results: example.com;\r\n\
            \tspf=pass smtp.mailfrom=sender@example.com;\r\n\
            \tdkim=pass header.d=example.com\r\n";
        let ar = parse(msg).unwrap().authentication_results.unwrap();
        assert!(ar.contains("spf=pass"));
        assert!(ar.contains("dkim=pass"));
    }

    #[test]
    fn parse_complainant_prefers_rcpt_to() {
        let msg = b"feedback-report\r\n\
            Original-Mail-From: a@x.com\r\n\
            Original-Rcpt-To: b@y.com\r\n";
        assert_eq!(parse(msg).unwrap().complainant(), Some("b@y.com"));
    }

    #[test]
    fn parse_complainant_fallback_to_mail_from() {
        let msg = b"feedback-report\r\nOriginal-Mail-From: a@x.com\r\n";
        assert_eq!(parse(msg).unwrap().complainant(), Some("a@x.com"));
    }

    #[test]
    fn parse_complainant_none_when_both_missing() {
        let msg = b"feedback-report\r\nFeedback-Type: abuse\r\n";
        assert_eq!(parse(msg).unwrap().complainant(), None);
    }

    #[test]
    fn parse_empty_input_returns_none() {
        assert!(parse(b"").is_none());
    }

    #[test]
    fn parse_short_no_match_returns_none() {
        assert!(parse(b"Hi").is_none());
    }

    #[test]
    fn parse_non_utf8_falls_through_to_no_headers() {
        // Has marker but binary garbage after — non-UTF-8 path
        // returns Some with all defaults rather than panicking.
        let mut msg = b"feedback-report\r\n".to_vec();
        msg.extend_from_slice(&[0xff, 0xfe, 0xfd, 0xfc]);
        let r = parse(&msg);
        assert!(r.is_some());
        assert_eq!(r.unwrap().feedback_type, "abuse");
    }

    #[test]
    fn parse_all_fields_at_once() {
        let msg = b"feedback-report\r\n\
            Feedback-Type: fraud\r\n\
            User-Agent: ACME FBL/1.0\r\n\
            Version: 1\r\n\
            Original-Mail-From: bad@x.com\r\n\
            Original-Rcpt-To: target@y.com\r\n\
            Arrival-Date: now\r\n\
            Source-IP: 10.0.0.1\r\n\
            Reported-Domain: x.com\r\n\
            Reported-URI: https://evil.example/landing\r\n\
            Authentication-Results: y.com; spf=fail\r\n\
            Incidents: 5\r\n";
        let r = parse(msg).unwrap();
        assert_eq!(r.feedback_type, "fraud");
        assert_eq!(r.user_agent.as_deref(), Some("ACME FBL/1.0"));
        assert_eq!(r.version.as_deref(), Some("1"));
        assert_eq!(r.original_mail_from.as_deref(), Some("bad@x.com"));
        assert_eq!(r.original_rcpt_to.as_deref(), Some("target@y.com"));
        assert_eq!(r.arrival_date.as_deref(), Some("now"));
        assert_eq!(r.source_ip.as_deref(), Some("10.0.0.1"));
        assert_eq!(r.reported_domain.as_deref(), Some("x.com"));
        assert_eq!(
            r.reported_uri.as_deref(),
            Some("https://evil.example/landing")
        );
        assert_eq!(
            r.authentication_results.as_deref(),
            Some("y.com; spf=fail")
        );
        assert_eq!(r.incidents.as_deref(), Some("5"));
    }
}
