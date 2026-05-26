//! Parsed SMTP reply from the wire — see [`parse_response`].

use compact_str::CompactString;

/// A parsed SMTP reply. `code` is the three-digit status (RFC 5321 §4.2.1);
/// `lines` is the human-readable text from each line, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtpResponse {
    /// the three-digit status code (e.g. 220, 250, 354, 550)
    pub code: u16,
    /// Reply text lines in order (one entry per wire line, without the
    /// code prefix).
    ///
    /// **v2 change**: `Vec<CompactString>` — EHLO extension lines
    /// (`PIPELINING`, `SIZE 52428800`, `STARTTLS`, `8BITMIME`,
    /// `AUTH PLAIN LOGIN`) and typical reply text all fit the 24-byte
    /// inline buffer, so the multi-line parse avoids one heap String
    /// alloc per line.
    pub lines: Vec<CompactString>,
}

impl SmtpResponse {
    /// `true` if the status code is in the 2xx or 3xx range (success or
    /// intermediate, e.g. 354 "start mail input").
    pub fn is_positive(&self) -> bool {
        (200..400).contains(&self.code)
    }

    /// `true` if the status code is in the 4xx range — caller should retry later.
    pub fn is_transient_error(&self) -> bool {
        (400..500).contains(&self.code)
    }

    /// `true` if the status code is 5xx — caller should not retry.
    pub fn is_permanent_error(&self) -> bool {
        self.code >= 500
    }

    /// Concatenate the reply lines with newlines for display / logging.
    pub fn message(&self) -> String {
        self.lines.join("\n")
    }

    /// Check if a specific EHLO extension keyword is advertised. Match is
    /// case-insensitive against the keyword segment of each line, before any
    /// space-separated parameters (e.g. `SIZE 10240000`).
    ///
    /// Byte-level compare — no per-call `to_uppercase` String allocations.
    pub fn has_extension(&self, keyword: &str) -> bool {
        let kw = keyword.as_bytes();
        self.lines.iter().any(|line| {
            let lb = line.as_bytes();
            if lb.len() < kw.len() {
                return false;
            }
            if !lb[..kw.len()].eq_ignore_ascii_case(kw) {
                return false;
            }
            // Either full-line match or followed by a space-separated param.
            lb.len() == kw.len() || lb[kw.len()] == b' '
        })
    }
}

/// parse a single or multiline SMTP response from raw text
/// returns None if the response is incomplete
pub fn parse_response(input: &str) -> Option<SmtpResponse> {
    // Pre-size to ~8 — typical EHLO responses are 4-12 lines
    // (server greeting + capability advertisements). Eliminates the
    // 4 → 8 growth tick we used to pay during 10-line EHLO parses.
    let mut lines: Vec<CompactString> = Vec::with_capacity(8);
    let mut code: Option<u16> = None;
    // Byte-level 3-digit decimal parse for the SMTP code. Skips
    // `<u16 as FromStr>`'s generic loop induction; each digit is
    // a single subtract + range check. ~5× faster than `.parse()` on
    // 3-byte input.
    #[inline]
    fn parse_code3(s: &[u8]) -> Option<u16> {
        let d0 = s[0].wrapping_sub(b'0');
        let d1 = s[1].wrapping_sub(b'0');
        let d2 = s[2].wrapping_sub(b'0');
        if d0 <= 9 && d1 <= 9 && d2 <= 9 {
            Some(d0 as u16 * 100 + d1 as u16 * 10 + d2 as u16)
        } else {
            None
        }
    }

    for line in input.lines() {
        let bytes = line.as_bytes();
        if bytes.len() < 3 {
            return None;
        }
        let line_code = parse_code3(&bytes[..3])?;

        if let Some(c) = code {
            if c != line_code {
                return None;
            }
        } else {
            code = Some(line_code);
        }

        let separator = bytes.get(3).copied();
        let text = if bytes.len() > 4 { &line[4..] } else { "" };
        lines.push(CompactString::new(text));

        // ' ' = last line, '-' = continuation
        match separator {
            Some(b' ') | None => {
                return Some(SmtpResponse { code: code?, lines });
            }
            Some(b'-') => continue,
            _ => return None,
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_line() {
        let r = parse_response("250 OK\r\n").unwrap();
        assert_eq!(r.code, 250);
        assert_eq!(r.lines, vec!["OK"]);
        assert!(r.is_positive());
    }

    #[test]
    fn parse_multiline() {
        let input = "250-mx.example.com\r\n250-PIPELINING\r\n250 SIZE 10240000";
        let r = parse_response(input).unwrap();
        assert_eq!(r.code, 250);
        assert_eq!(r.lines.len(), 3);
        assert_eq!(r.lines[0], "mx.example.com");
        assert_eq!(r.lines[2], "SIZE 10240000");
    }

    #[test]
    fn transient_error() {
        let r = parse_response("421 Try again later").unwrap();
        assert!(r.is_transient_error());
        assert!(!r.is_positive());
    }

    #[test]
    fn permanent_error() {
        let r = parse_response("550 User not found").unwrap();
        assert!(r.is_permanent_error());
    }

    #[test]
    fn incomplete_returns_none() {
        assert!(parse_response("").is_none());
    }

    #[test]
    fn has_extension_starttls() {
        let r = parse_response(
            "250-mx.example.com\r\n250-PIPELINING\r\n250-STARTTLS\r\n250 SIZE 10240000",
        )
        .unwrap();
        assert!(r.has_extension("STARTTLS"));
        assert!(r.has_extension("starttls"));
        assert!(r.has_extension("PIPELINING"));
    }

    #[test]
    fn has_extension_with_params() {
        let r = parse_response("250-mx.example.com\r\n250 SIZE 10240000").unwrap();
        assert!(r.has_extension("SIZE"));
        assert!(!r.has_extension("STARTTLS"));
    }

    #[test]
    fn has_extension_case_insensitive() {
        let r = parse_response("250-mx.example.com\r\n250 starttls").unwrap();
        assert!(r.has_extension("STARTTLS"));
        assert!(r.has_extension("Starttls"));
    }

    #[test]
    fn has_extension_no_partial_match() {
        // "STARTTLSPLUS" should not match "STARTTLS"
        let r = SmtpResponse {
            code: 250,
            lines: vec!["mx.example.com".into(), "STARTTLSPLUS".into()],
        };
        assert!(!r.has_extension("STARTTLS"));
    }

    #[test]
    fn has_extension_empty_lines() {
        let r = SmtpResponse {
            code: 250,
            lines: vec![],
        };
        assert!(!r.has_extension("STARTTLS"));
    }

    // --- additional edge-case tests ---

    #[test]
    fn parse_response_invalid_code_letters() {
        assert!(parse_response("abc OK").is_none());
    }

    #[test]
    fn parse_response_short_line_two_chars() {
        assert!(parse_response("25").is_none());
    }

    #[test]
    fn parse_response_exactly_three_chars() {
        // "250" with no separator — treated as final line (separator = None)
        let r = parse_response("250").unwrap();
        assert_eq!(r.code, 250);
        assert_eq!(r.lines, vec![""]);
    }

    #[test]
    fn parse_response_code_only_with_space() {
        // "250 " — code + space + empty text
        let r = parse_response("250 ").unwrap();
        assert_eq!(r.code, 250);
        assert_eq!(r.lines, vec![""]);
    }

    #[test]
    fn parse_response_mismatched_codes() {
        // first line says 250, second says 451
        assert!(parse_response("250-ok\r\n451 fail").is_none());
    }

    #[test]
    fn parse_response_invalid_separator() {
        // '=' is not a valid separator
        assert!(parse_response("250=OK").is_none());
    }

    #[test]
    fn parse_response_only_continuation_never_final() {
        // all continuation lines, no final line
        assert!(parse_response("250-line1\r\n250-line2").is_none());
    }

    #[test]
    fn parse_response_greeting_220() {
        let r = parse_response("220 mx.example.com ESMTP ready").unwrap();
        assert_eq!(r.code, 220);
        assert!(r.is_positive());
        assert_eq!(r.lines, vec!["mx.example.com ESMTP ready"]);
    }

    #[test]
    fn parse_response_354_go_ahead() {
        let r = parse_response("354 Start mail input; end with <CRLF>.<CRLF>").unwrap();
        assert_eq!(r.code, 354);
        assert!(r.is_positive());
    }

    #[test]
    fn is_positive_boundary_199() {
        let r = SmtpResponse {
            code: 199,
            lines: vec![],
        };
        assert!(!r.is_positive());
    }

    #[test]
    fn is_positive_boundary_200() {
        let r = SmtpResponse {
            code: 200,
            lines: vec![],
        };
        assert!(r.is_positive());
    }

    #[test]
    fn is_positive_boundary_399() {
        let r = SmtpResponse {
            code: 399,
            lines: vec![],
        };
        assert!(r.is_positive());
    }

    #[test]
    fn is_positive_boundary_400() {
        let r = SmtpResponse {
            code: 400,
            lines: vec![],
        };
        assert!(!r.is_positive());
    }

    #[test]
    fn is_transient_boundary_399() {
        let r = SmtpResponse {
            code: 399,
            lines: vec![],
        };
        assert!(!r.is_transient_error());
    }

    #[test]
    fn is_transient_boundary_400() {
        let r = SmtpResponse {
            code: 400,
            lines: vec![],
        };
        assert!(r.is_transient_error());
    }

    #[test]
    fn is_transient_boundary_499() {
        let r = SmtpResponse {
            code: 499,
            lines: vec![],
        };
        assert!(r.is_transient_error());
    }

    #[test]
    fn is_transient_boundary_500() {
        let r = SmtpResponse {
            code: 500,
            lines: vec![],
        };
        assert!(!r.is_transient_error());
    }

    #[test]
    fn is_permanent_boundary_499() {
        let r = SmtpResponse {
            code: 499,
            lines: vec![],
        };
        assert!(!r.is_permanent_error());
    }

    #[test]
    fn is_permanent_boundary_500() {
        let r = SmtpResponse {
            code: 500,
            lines: vec![],
        };
        assert!(r.is_permanent_error());
    }

    #[test]
    fn is_permanent_high_code() {
        let r = SmtpResponse {
            code: 599,
            lines: vec![],
        };
        assert!(r.is_permanent_error());
    }

    #[test]
    fn message_joins_lines() {
        let r = SmtpResponse {
            code: 250,
            lines: vec!["line1".into(), "line2".into(), "line3".into()],
        };
        assert_eq!(r.message(), "line1\nline2\nline3");
    }

    #[test]
    fn message_single_line() {
        let r = SmtpResponse {
            code: 250,
            lines: vec!["OK".into()],
        };
        assert_eq!(r.message(), "OK");
    }

    #[test]
    fn message_empty() {
        let r = SmtpResponse {
            code: 250,
            lines: vec![],
        };
        assert_eq!(r.message(), "");
    }

    #[test]
    fn parse_multiline_ehlo_full() {
        let input = "250-mail.example.com Hello\r\n\
                      250-SIZE 52428800\r\n\
                      250-8BITMIME\r\n\
                      250-AUTH LOGIN PLAIN\r\n\
                      250-ENHANCEDSTATUSCODES\r\n\
                      250-PIPELINING\r\n\
                      250-CHUNKING\r\n\
                      250 SMTPUTF8";
        let r = parse_response(input).unwrap();
        assert_eq!(r.code, 250);
        assert_eq!(r.lines.len(), 8);
        assert!(r.has_extension("8BITMIME"));
        assert!(r.has_extension("AUTH"));
        assert!(r.has_extension("SMTPUTF8"));
        assert!(r.has_extension("CHUNKING"));
        assert!(!r.has_extension("VRFY"));
    }

    #[test]
    fn has_extension_exact_keyword_with_space_param() {
        // "AUTH LOGIN PLAIN" should match "AUTH" but not "LOGIN"
        let r = SmtpResponse {
            code: 250,
            lines: vec!["AUTH LOGIN PLAIN".into()],
        };
        assert!(r.has_extension("AUTH"));
        assert!(!r.has_extension("LOGIN"));
    }

    #[test]
    fn parse_response_empty_text_in_multiline() {
        let input = "250-\r\n250 OK";
        let r = parse_response(input).unwrap();
        assert_eq!(r.code, 250);
        assert_eq!(r.lines, vec!["", "OK"]);
    }

    #[test]
    fn smtp_response_clone_and_eq() {
        let r1 = SmtpResponse {
            code: 250,
            lines: vec!["OK".into()],
        };
        let r2 = r1.clone();
        assert_eq!(r1, r2);
    }

    #[test]
    fn smtp_response_debug() {
        let r = SmtpResponse {
            code: 550,
            lines: vec!["no such user".into()],
        };
        let debug = format!("{:?}", r);
        assert!(debug.contains("550"));
        assert!(debug.contains("no such user"));
    }

    // --- new tests ---

    #[test]
    fn parse_response_multiline_with_lf_only() {
        // some servers send LF instead of CRLF
        let input = "250-mx.example.com\n250-PIPELINING\n250 OK";
        let r = parse_response(input).unwrap();
        assert_eq!(r.code, 250);
        assert_eq!(r.lines.len(), 3);
        assert_eq!(r.lines[2], "OK");
    }

    #[test]
    fn parse_response_code_451_service_unavailable() {
        let r = parse_response("451 Requested action aborted: local error").unwrap();
        assert_eq!(r.code, 451);
        assert!(r.is_transient_error());
        assert!(!r.is_positive());
        assert!(!r.is_permanent_error());
    }

    #[test]
    fn parse_response_code_553_syntax() {
        let r = parse_response("553 5.1.3 Invalid address format").unwrap();
        assert_eq!(r.code, 553);
        assert!(r.is_permanent_error());
    }

    #[test]
    fn parse_response_multiline_four_lines() {
        let input = "220-mx1.example.com ESMTP\r\n\
                      220-Service ready\r\n\
                      220-No soliciting\r\n\
                      220 Ready";
        let r = parse_response(input).unwrap();
        assert_eq!(r.code, 220);
        assert_eq!(r.lines.len(), 4);
    }

    #[test]
    fn parse_response_long_text() {
        let long_text = "x".repeat(500);
        let input = format!("250 {long_text}");
        let r = parse_response(&input).unwrap();
        assert_eq!(r.code, 250);
        assert_eq!(r.lines[0], long_text);
    }

    #[test]
    fn has_extension_8bitmime() {
        let r = SmtpResponse {
            code: 250,
            lines: vec!["mx.example.com".into(), "8BITMIME".into()],
        };
        assert!(r.has_extension("8BITMIME"));
        assert!(r.has_extension("8bitmime"));
    }

    #[test]
    fn has_extension_auth_with_mechanisms() {
        let r = SmtpResponse {
            code: 250,
            lines: vec!["mx.example.com".into(), "AUTH LOGIN PLAIN XOAUTH2".into()],
        };
        assert!(r.has_extension("AUTH"));
        // "PLAIN" is a parameter, not a keyword
        assert!(!r.has_extension("PLAIN"));
        assert!(!r.has_extension("XOAUTH2"));
    }

    #[test]
    fn categories_mutually_exclusive_for_standard_codes() {
        // a 2xx code is positive only
        let r2 = SmtpResponse {
            code: 250,
            lines: vec![],
        };
        assert!(r2.is_positive());
        assert!(!r2.is_transient_error());
        assert!(!r2.is_permanent_error());

        // a 4xx code is transient only
        let r4 = SmtpResponse {
            code: 450,
            lines: vec![],
        };
        assert!(!r4.is_positive());
        assert!(r4.is_transient_error());
        assert!(!r4.is_permanent_error());

        // a 5xx code is permanent only
        let r5 = SmtpResponse {
            code: 550,
            lines: vec![],
        };
        assert!(!r5.is_positive());
        assert!(!r5.is_transient_error());
        assert!(r5.is_permanent_error());
    }

    #[test]
    fn parse_response_code_221_bye() {
        let r = parse_response("221 2.0.0 Bye").unwrap();
        assert_eq!(r.code, 221);
        assert!(r.is_positive());
        assert_eq!(r.lines, vec!["2.0.0 Bye"]);
    }

    #[test]
    fn parse_response_code_235_auth_success() {
        let r = parse_response("235 2.7.0 Authentication successful").unwrap();
        assert_eq!(r.code, 235);
        assert!(r.is_positive());
    }

    #[test]
    fn parse_response_code_535_auth_failed() {
        let r = parse_response("535 5.7.8 Authentication credentials invalid").unwrap();
        assert_eq!(r.code, 535);
        assert!(r.is_permanent_error());
    }

    #[test]
    fn message_preserves_whitespace() {
        let r = SmtpResponse {
            code: 250,
            lines: vec!["  leading spaces".into(), "trailing spaces  ".into()],
        };
        assert_eq!(r.message(), "  leading spaces\ntrailing spaces  ");
    }
}
