//! `Authentication-Results:` header fields that name a given authserv-id
//! (RFC 8601 §5): finding, removing, and telling apart the one a server
//! wrote itself from one a sender wrote in its name.
//!
//! A server that writes these must remove every incoming one that claims
//! its authserv-id, on every path a message can enter a mailbox by, or a
//! sender writes `dkim=pass` for it.

/// The authserv-id of an `Authentication-Results:` value: the first
/// token before `;`, without the optional version number.
#[must_use]
pub fn authserv_id(value: &str) -> &str {
    value
        .split(';')
        .next()
        .unwrap_or("")
        .split_whitespace()
        .next()
        .unwrap_or("")
}

/// Whether any header field is an `Authentication-Results:` naming
/// `authserv` (any case), so [`strip_claiming`] has work to do.
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

/// `raw` without the header fields that are an `Authentication-Results:`
/// naming `authserv`, continuation lines included. Every other byte, the
/// body included, is kept as it was.
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

/// `raw` as it may be filed: unchanged unless a header field claims
/// `authserv`, which is then removed. `None` for `authserv` (the server
/// does not know its own name) leaves it unchanged.
#[must_use]
pub fn without_claiming<'a>(raw: &'a [u8], authserv: Option<&str>) -> std::borrow::Cow<'a, [u8]> {
    match authserv {
        Some(ours) if has_claiming(raw, ours) => std::borrow::Cow::Owned(strip_claiming(raw, ours)),
        _ => std::borrow::Cow::Borrowed(raw),
    }
}

/// The value of the `Authentication-Results:` that `authserv`'s own
/// receiver wrote, unfolded; `None` when the message does not carry one.
///
/// The receiver puts its header first and its own trace directly below
/// it (`Received: from … by <authserv> with ESMTP`), above everything the
/// sender wrote. A header naming `authserv` anywhere else, or one not
/// followed by that trace, was written by somebody else, typically
/// carried in by an import from before such headers were removed on the
/// way in.
#[must_use]
pub fn written_by(raw: &[u8], authserv: &str) -> Option<String> {
    let end = header_end(raw);
    let mut fields: Vec<String> = Vec::with_capacity(2);
    for line in raw[..end].split_inclusive(|&b| b == b'\n') {
        let text = String::from_utf8_lossy(line);
        if continues(line) {
            if let Some(f) = fields.last_mut() {
                f.push_str(&text);
            }
            continue;
        }
        if fields.len() == 2 {
            break;
        }
        fields.push(text.into_owned());
    }
    let [first, second] = fields.as_slice() else {
        return None;
    };
    let value = strip_name(first.as_bytes())
        .map(|v| String::from_utf8_lossy(v).replace(['\r', '\n', '\t'], " "))?;
    if !authserv_id(&value).eq_ignore_ascii_case(authserv) {
        return None;
    }
    let trace = second.replace(['\r', '\n', '\t'], " ").to_ascii_lowercase();
    let ours = format!(" by {} with esmtp", authserv.to_ascii_lowercase());
    (trace.starts_with("received:") && trace.contains(&ours)).then_some(value)
}

/// Whether the first `Authentication-Results:` names `authserv` although
/// that server's receiver did not write it ([`written_by`]): a header a
/// reader must disregard. Every other header is read as before, and with
/// no `authserv` nothing counts as forged.
#[must_use]
pub fn first_is_forged(raw: &[u8], authserv: Option<&str>) -> bool {
    let Some(us) = authserv else {
        return false;
    };
    first_value(raw).is_some_and(|v| authserv_id(&v).eq_ignore_ascii_case(us))
        && written_by(raw, us).is_none()
}

/// The first `Authentication-Results:` value, unfolded.
fn first_value(raw: &[u8]) -> Option<String> {
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
    value.map(|v| v.replace(['\r', '\n', '\t'], " "))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_claiming_our_id_is_stripped_and_others_kept() {
        let raw = b"Received: from a\r\n\tby b\r\n\
Authentication-Results: MAIL.golia.ai; dkim=pass\r\n\theader.d=stripe.com\r\n\
Authentication-Results:\r\n mail.golia.ai;\r\n dkim=pass header.d=x.com\r\n\
Authentication-Results: mx.google.com; dkim=pass header.d=stripe.com\r\n\
From: a@stripe.com\r\n\r\n\
Authentication-Results: mail.golia.ai; in the body\r\n";
        assert!(has_claiming(raw, "mail.golia.ai"));
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
        assert!(!has_claiming(raw, "mail.golia.ai"));
        assert_eq!(strip_claiming(raw, "mail.golia.ai"), raw.to_vec());
        assert!(matches!(
            without_claiming(raw, Some("mail.golia.ai")),
            std::borrow::Cow::Borrowed(_)
        ));
        assert!(matches!(
            without_claiming(b"Authentication-Results: x; spf=pass\r\n\r\n", None),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    const STAMPED: &str = "Authentication-Results: mail.golia.ai;\r\n\tspf=pass;\r\n\tdkim=pass header.d=stripe.com\r\n\
Received: from mx.stripe.com (1.2.3.4:5)\r\n\tby mail.golia.ai with ESMTP\r\n\tfor <a@golia.jp>; Mon, 2 Nov 2026 00:00:00 +0000\r\n\
From: a@stripe.com\r\n\r\nbody\r\n";

    #[test]
    fn our_receivers_header_is_ours() {
        let v = written_by(STAMPED.as_bytes(), "mail.golia.ai").unwrap();
        assert!(v.contains("dkim=pass header.d=stripe.com"), "{v}");
    }

    #[test]
    fn a_header_naming_us_without_our_trace_below_it_is_not_ours() {
        let no_trace = "Authentication-Results: mail.golia.ai; dkim=pass header.d=stripe.com\r\n\
From: a@stripe.com\r\n\r\nbody\r\n";
        assert_eq!(written_by(no_trace.as_bytes(), "mail.golia.ai"), None);
        let someone_elses_trace = "Authentication-Results: mail.golia.ai; dkim=pass header.d=stripe.com\r\n\
Received: from x by mx.google.com with ESMTPS\r\n\r\nbody\r\n";
        assert_eq!(
            written_by(someone_elses_trace.as_bytes(), "mail.golia.ai"),
            None
        );
        let not_first = format!("Received: from y by mx.google.com\r\n{STAMPED}");
        assert_eq!(written_by(not_first.as_bytes(), "mail.golia.ai"), None);
        assert_eq!(written_by(STAMPED.as_bytes(), "mx.other.example"), None);
    }

    #[test]
    fn only_a_first_header_naming_us_that_we_did_not_write_is_forged() {
        let us = Some("mail.golia.ai");
        assert!(!first_is_forged(STAMPED.as_bytes(), us), "ours");
        let no_trace = b"Authentication-Results: mail.golia.ai; dmarc=pass\r\nFrom: a@b.c\r\n\r\nx";
        assert!(first_is_forged(no_trace, us));
        assert!(
            !first_is_forged(no_trace, None),
            "without our name nothing is forged"
        );
        let google = b"Authentication-Results: mx.google.com; dmarc=pass\r\nFrom: a@b.c\r\n\r\nx";
        assert!(
            !first_is_forged(google, us),
            "another server's is read as before"
        );
        assert!(!first_is_forged(b"From: a@b.c\r\n\r\nx", us), "none at all");
    }
}
