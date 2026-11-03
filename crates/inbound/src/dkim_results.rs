//! DKIM results per signature in the `Authentication-Results:` this
//! server writes, and only this server's.
//!
//! One word for all signatures (`dkim=pass`) cannot say whose signature
//! passed, so a sender whose own aligned signature verified beside a
//! sending service's (Amazon SES, Marketo) looked no different from one
//! whose only passing signature was the service's. RFC 8601 lets each
//! signature have its own entry with `header.d=` and `header.s=`.
//!
//! An entry is only evidence when this server wrote it. Incoming headers
//! that claim our authserv-id are removed on every way in
//! ([`mailrs_rfc5322::auth_results::strip_claiming`]), and the reader
//! trusts only the header our receiver put on top of its own trace
//! ([`mailrs_rfc5322::auth_results::written_by`]).

use crate::context::AuthResults;

/// One DKIM signature's own verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DkimSignature {
    /// RFC 8601 result token: `pass`, `fail`, `neutral`, `policy`,
    /// `temperror`, `permerror`.
    pub result: String,
    /// The signature's `d=`, lowercased; empty when it did not parse.
    pub domain: String,
    /// The signature's `s=`; empty when it did not parse.
    pub selector: String,
}

/// The `Authentication-Results:` header, with one `dkim=` entry per
/// signature (`dkim=pass header.d=stripe.com header.s=s1`), or the single
/// aggregate `dkim=` when no signature was checked.
#[must_use]
pub fn build_auth_header(hostname: &str, auth: &AuthResults, dmarc_reason: Option<&str>) -> String {
    let mut out = String::with_capacity(256);
    out.push_str("Authentication-Results: ");
    out.push_str(hostname);
    out.push_str(";\r\n\tspf=");
    out.push_str(&auth.spf);
    if auth.dkim_signatures.is_empty() {
        out.push_str(";\r\n\tdkim=");
        out.push_str(&auth.dkim);
    }
    for sig in &auth.dkim_signatures {
        out.push_str(";\r\n\tdkim=");
        out.push_str(&sig.result);
        // `d=` and `s=` come from the sender's own header: written only
        // when they are plain domain tokens, never anything that could
        // end the entry or the field
        if is_token(&sig.domain) {
            out.push_str(" header.d=");
            out.push_str(&sig.domain);
        }
        if is_token(&sig.selector) {
            out.push_str(" header.s=");
            out.push_str(&sig.selector);
        }
    }
    out.push_str(";\r\n\tarc=");
    out.push_str(&auth.arc);
    out.push_str(";\r\n\tdmarc=");
    out.push_str(&auth.dmarc);
    if let Some(reason) = dmarc_reason {
        out.push_str(" reason=\"");
        out.push_str(reason);
        out.push('"');
    }
    out.push_str("\r\n");
    out
}

fn is_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// `(result, d=)` of each DKIM entry our receiver wrote on the message.
///
/// `None` when the message carries no header our receiver wrote, or the
/// one it carries records one result for all signatures (no entry has
/// `header.d=`), which is how mail received before entries per signature
/// was recorded.
#[must_use]
pub fn ours(raw: &[u8], authserv: &str) -> Option<Vec<(String, String)>> {
    let value = mailrs_rfc5322::auth_results::written_by(raw, authserv)?;
    let entries: Vec<(String, String)> = value
        .split(';')
        .filter_map(|seg| {
            let rest = seg.trim().strip_prefix("dkim=")?;
            let mut words = rest.split_whitespace();
            let result = words.next()?.to_ascii_lowercase();
            let d = words.find_map(|w| w.strip_prefix("header.d="))?;
            Some((result, d.trim_matches('"').to_ascii_lowercase()))
        })
        .collect();
    (!entries.is_empty()).then_some(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(result: &str, domain: &str, selector: &str) -> DkimSignature {
        DkimSignature {
            result: result.into(),
            domain: domain.into(),
            selector: selector.into(),
        }
    }

    fn auth(sigs: Vec<DkimSignature>) -> AuthResults {
        AuthResults {
            spf: "pass".into(),
            dkim: "pass".into(),
            dmarc: "pass".into(),
            dkim_signatures: sigs,
            ..AuthResults::default()
        }
    }

    /// What the receiver delivers: its header, then its own trace.
    fn delivered(header: &str) -> String {
        format!(
            "{header}Received: from mx.stripe.com (1.2.3.4:5)\r\n\tby mail.golia.ai with ESMTP\r\n\tfor <a@golia.jp>; Mon, 2 Nov 2026 00:00:00 +0000\r\nFrom: a@stripe.com\r\n\r\nbody\r\n"
        )
    }

    #[test]
    fn each_signature_gets_its_own_entry() {
        let h = build_auth_header(
            "mail.golia.ai",
            &auth(vec![
                sig("pass", "stripe.com", "s1"),
                sig("pass", "amazonses.com", "ug7"),
            ]),
            None,
        );
        assert!(
            h.contains("dkim=pass header.d=stripe.com header.s=s1;"),
            "{h}"
        );
        assert!(
            h.contains("dkim=pass header.d=amazonses.com header.s=ug7;"),
            "{h}"
        );
        assert!(h.ends_with("dmarc=pass\r\n"), "{h}");
    }

    #[test]
    fn no_signature_keeps_the_single_entry() {
        let mut a = auth(vec![]);
        a.dkim = "none".into();
        let h = build_auth_header("mail.golia.ai", &a, Some("policy=none"));
        assert!(h.contains("\tdkim=none;"), "{h}");
        assert!(h.contains("reason=\"policy=none\""), "{h}");
    }

    #[test]
    fn a_hostile_d_or_s_is_left_out() {
        let h = build_auth_header(
            "mail.golia.ai",
            &auth(vec![sig(
                "pass",
                "x.com; dkim=pass header.d=stripe.com",
                "a\r\nb",
            )]),
            None,
        );
        assert!(!h.contains("header.d="), "{h}");
        assert!(!h.contains("header.s="), "{h}");
        assert!(h.contains("\tdkim=pass;"), "{h}");
    }

    #[test]
    fn our_own_header_reads_back() {
        let h = build_auth_header(
            "mail.golia.ai",
            &auth(vec![
                sig("fail", "stripe.com", "s1"),
                sig("pass", "amazonses.com", "ug7"),
            ]),
            None,
        );
        assert_eq!(
            ours(delivered(&h).as_bytes(), "mail.golia.ai"),
            Some(vec![
                ("fail".to_string(), "stripe.com".to_string()),
                ("pass".to_string(), "amazonses.com".to_string()),
            ])
        );
    }

    #[test]
    fn a_single_word_header_says_nothing_per_signature() {
        let raw = delivered(
            "Authentication-Results: mail.golia.ai;\r\n\tspf=pass;\r\n\tdkim=pass;\r\n\tdmarc=pass\r\n",
        );
        assert_eq!(ours(raw.as_bytes(), "mail.golia.ai"), None);
    }

    #[test]
    fn someone_elses_header_is_not_read() {
        let raw = b"Authentication-Results: mx.google.com; dkim=pass header.d=stripe.com\r\n\r\nx";
        assert_eq!(ours(raw, "mail.golia.ai"), None);
    }

    #[test]
    fn a_header_naming_us_without_our_trace_is_not_read() {
        let raw = b"Authentication-Results: mail.golia.ai; dkim=pass header.d=stripe.com\r\nFrom: a@stripe.com\r\n\r\nx";
        assert_eq!(ours(raw, "mail.golia.ai"), None);
    }
}
