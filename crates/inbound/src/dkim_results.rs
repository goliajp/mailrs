//! DKIM results per signature in the `Authentication-Results:` this
//! server writes, and only this server's.
//!
//! One word for all signatures (`dkim=pass`) cannot say whose signature
//! passed, so a sender whose own aligned signature verified beside a
//! sending service's (Amazon SES, Marketo) looked no different from one
//! whose only passing signature was the service's. RFC 8601 lets each
//! signature have its own entry with `header.d=` and `header.s=`.
//!
//! An entry is only evidence when this server wrote it. A sender can put
//! `Authentication-Results: <our host>; dkim=pass header.d=stripe.com`
//! in the message itself, so every incoming header that claims our
//! authserv-id is removed before ours is added (RFC 8601 §5), and the
//! reader trusts only the first header, ours, when it names our host.

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

/// Whether any header field claims to be this server's
/// `Authentication-Results:`, so [`strip_claiming`] has work to do.
#[must_use]
pub fn has_claiming(raw: &[u8], authserv: &str) -> bool {
    let end = header_end(raw);
    let mut pos = 0;
    for line in raw[..end].split_inclusive(|&b| b == b'\n') {
        if !continues(line) && field_claims(&raw[pos..end], authserv) {
            return true;
        }
        pos += line.len();
    }
    false
}

/// `raw` without the header fields that claim to be this server's
/// `Authentication-Results:` (authserv-id equal to `authserv`, any case).
/// Every other byte, the body included, is kept as it was.
#[must_use]
pub fn strip_claiming(raw: &[u8], authserv: &str) -> Vec<u8> {
    let end = header_end(raw);
    let mut out = Vec::with_capacity(raw.len());
    let mut dropping = false;
    let mut pos = 0;
    for line in raw[..end].split_inclusive(|&b| b == b'\n') {
        if !continues(line) {
            dropping = field_claims(&raw[pos..end], authserv);
        }
        if !dropping {
            out.extend_from_slice(line);
        }
        pos += line.len();
    }
    out.extend_from_slice(&raw[end..]);
    out
}

fn continues(line: &[u8]) -> bool {
    line.first().is_some_and(|&b| b == b' ' || b == b'\t')
}

/// Whether the header field at the start of `field` (its first line,
/// then its continuation lines) is an `Authentication-Results:` naming
/// `authserv`. The id may sit on a continuation line.
fn field_claims(field: &[u8], authserv: &str) -> bool {
    let mut lines = field.split_inclusive(|&b| b == b'\n');
    let Some(rest) = lines.next().and_then(strip_name) else {
        return false;
    };
    let mut value = String::from_utf8_lossy(rest).into_owned();
    for line in lines.take_while(|l| continues(l)) {
        if value.contains(';') {
            break;
        }
        value.push_str(&String::from_utf8_lossy(line));
    }
    authserv_id(&value.replace(['\r', '\n', '\t'], " ")).eq_ignore_ascii_case(authserv)
}

fn strip_name(line: &[u8]) -> Option<&[u8]> {
    const NAME: &[u8] = b"authentication-results:";
    (line.len() >= NAME.len() && line[..NAME.len()].eq_ignore_ascii_case(NAME))
        .then(|| &line[NAME.len()..])
}

/// The authserv-id of an `Authentication-Results:` value: the first
/// token before `;`, without the optional version number.
fn authserv_id(value: &str) -> &str {
    value
        .split(';')
        .next()
        .unwrap_or("")
        .split_whitespace()
        .next()
        .unwrap_or("")
}

fn header_end(raw: &[u8]) -> usize {
    let crlf = raw.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 2);
    let lf = raw.windows(2).position(|w| w == b"\n\n").map(|p| p + 1);
    match (crlf, lf) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => raw.len(),
    }
}

/// `(result, d=)` of each DKIM entry this server wrote on the message.
///
/// Read from the first `Authentication-Results:` only, and only when it
/// names `authserv`: that is the one the receiver put on top. `None` when
/// there is no such header or it records one result for all signatures
/// (no entry has `header.d=`), which is how mail received before entries
/// per signature was recorded.
#[must_use]
pub fn ours(raw: &[u8], authserv: &str) -> Option<Vec<(String, String)>> {
    let end = header_end(raw);
    let mut value: Option<String> = None;
    for line in raw[..end].split_inclusive(|&b| b == b'\n') {
        match (&mut value, continues(line)) {
            (Some(v), true) => v.push_str(&String::from_utf8_lossy(line)),
            (Some(_), false) => break,
            (None, _) => {
                if let Some(rest) = strip_name(line) {
                    value = Some(String::from_utf8_lossy(rest).into_owned());
                }
            }
        }
    }
    let value = value?.replace(['\r', '\n', '\t'], " ");
    if !authserv_id(&value).eq_ignore_ascii_case(authserv) {
        return None;
    }
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
        let raw = format!("{h}From: a@stripe.com\r\n\r\nbody\r\n");
        assert_eq!(
            ours(raw.as_bytes(), "mail.golia.ai"),
            Some(vec![
                ("fail".to_string(), "stripe.com".to_string()),
                ("pass".to_string(), "amazonses.com".to_string()),
            ])
        );
    }

    #[test]
    fn a_single_word_header_says_nothing_per_signature() {
        let raw = b"Authentication-Results: mail.golia.ai;\r\n\tspf=pass;\r\n\tdkim=pass;\r\n\tdmarc=pass\r\nFrom: a@b.com\r\n\r\nx";
        assert_eq!(ours(raw, "mail.golia.ai"), None);
    }

    #[test]
    fn someone_elses_header_is_not_read() {
        let raw = b"Authentication-Results: mx.google.com; dkim=pass header.d=stripe.com\r\n\r\nx";
        assert_eq!(ours(raw, "mail.golia.ai"), None);
    }

    #[test]
    fn a_header_claiming_our_id_is_stripped_and_others_kept() {
        let raw = b"Received: from a\r\n\tby b\r\n\
Authentication-Results: MAIL.golia.ai; dkim=pass\r\n\theader.d=stripe.com\r\n\
Authentication-Results:\r\n mail.golia.ai;\r\n dkim=pass header.d=x.com\r\n\
Authentication-Results: mx.google.com; dkim=pass header.d=stripe.com\r\n\
From: a@stripe.com\r\n\r\n\
Authentication-Results: mail.golia.ai; in the body\r\n";
        let out = String::from_utf8(strip_claiming(raw, "mail.golia.ai")).unwrap();
        assert_eq!(
            out,
            "Received: from a\r\n\tby b\r\n\
Authentication-Results: mx.google.com; dkim=pass header.d=stripe.com\r\n\
From: a@stripe.com\r\n\r\n\
Authentication-Results: mail.golia.ai; in the body\r\n"
        );
    }

    #[test]
    fn a_message_without_one_is_unchanged() {
        let raw = b"From: a@b.com\nSubject: x\n\nbody\n";
        assert_eq!(strip_claiming(raw, "mail.golia.ai"), raw.to_vec());
    }
}
