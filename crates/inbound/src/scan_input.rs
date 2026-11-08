//! What the reputation judgement and the attachment scan read out of a
//! message. One copy, for the receiver, the historical sweep and the
//! replay: a second reading of "which parts are attachments" or "which
//! hosts the links go to" would let one of them see a different letter.

/// One attachment: its name, its declared type lowercased, its bytes.
pub struct Part<'p> {
    /// File name, or empty.
    pub name: String,
    /// Content type, lowercased.
    pub declared_type: String,
    /// Decoded bytes.
    pub bytes: &'p [u8],
}

/// Every part a reader can save as a file.
pub fn attachments<'p>(parsed: &'p mailrs_mime::Part<'_>) -> Vec<Part<'p>> {
    parsed
        .attachments()
        .map(|p| Part {
            name: p.attachment_filename().unwrap_or("").to_string(),
            declared_type: p.content_type.mime_type().to_ascii_lowercase(),
            bytes: &p.body,
        })
        .collect()
}

/// The hosts the message's links go to: every `http`/`https` address in
/// the text parts and every `href` in the HTML ones, lowercased, without
/// repeats, in the order first met. At most 200.
pub fn link_hosts(parsed: &mailrs_mime::Part<'_>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in parsed
        .walk()
        .filter(|p| p.children.is_empty() && !p.is_attachment())
    {
        let Some(text) = p.body_text() else {
            continue;
        };
        let lower = text.to_ascii_lowercase();
        let doctype = doctype_span(&lower);
        let mut from = 0;
        while let Some(at) = lower[from..].find("http") {
            let after = from + at + 4;
            let skip = match &lower[after..] {
                r if r.starts_with("://") => Some(3),
                r if r.starts_with("s://") => Some(4),
                _ => None,
            };
            let start = from + at;
            from = after;
            let Some(skip) = skip else {
                continue;
            };
            if doctype.contains(&start) || is_a_namespace_value(&lower[..start]) {
                continue;
            }
            if let Some(host) = host_at(&lower[after + skip..])
                && !out.contains(&host)
                && out.len() < 200
            {
                out.push(host);
            }
        }
    }
    out
}

/// What a reader's Junk verdict on a conversation points at: the domains
/// its messages came from and the hosts their links go to, lowercased,
/// without repeats. Messages the reader sent themself are left out.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct JunkSubjects {
    /// From-header domains.
    pub senders: Vec<String>,
    /// Link hosts, at most 20.
    pub links: Vec<String>,
}

/// Read `raws`, the messages of one conversation `user` filed as Junk.
pub fn junk_subjects(raws: &[Vec<u8>], user: &str) -> JunkSubjects {
    let mut out = JunkSubjects::default();
    for raw in raws {
        let from = crate::identity::from_address(raw).to_ascii_lowercase();
        if from.is_empty() || from.eq_ignore_ascii_case(user) {
            continue;
        }
        if let Some((_, domain)) = from.rsplit_once('@')
            && !domain.is_empty()
            && !out.senders.iter().any(|d| d == domain)
        {
            out.senders.push(domain.to_string());
        }
        for host in link_hosts(&mailrs_mime::parse(raw)) {
            if out.links.len() < 20 && !out.links.contains(&host) {
                out.links.push(host);
            }
        }
    }
    out
}

/// Where `<!doctype …>` sits in `lower`, or an empty range. XHTML names
/// its DTD with a URL there, which is an identifier, not a link.
fn doctype_span(lower: &str) -> std::ops::Range<usize> {
    let Some(open) = lower.find("<!doctype") else {
        return 0..0;
    };
    let close = lower[open..].find('>').map_or(lower.len(), |i| open + i);
    open..close
}

/// Whether the URL that starts right after `before` is the value of an
/// `xmlns` / `xmlns:*` attribute. Word and Outlook HTML declare
/// `www.w3.org`, `schemas.microsoft.com` and `schemas.openxmlformats.org`
/// as namespaces, and counting them would make every such message look
/// as if it linked somewhere. Decided by where the URL stands, not by its
/// host, so the same hosts in an `href` or in prose are still links.
/// `before` is lowercased.
fn is_a_namespace_value(before: &str) -> bool {
    let Some(value_start) = before.strip_suffix(['"', '\'']) else {
        return false;
    };
    let Some(name_end) = value_start.trim_end().strip_suffix('=') else {
        return false;
    };
    let name_end = name_end.trim_end();
    let name_start = name_end
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '.' | '-')))
        .map_or(0, |i| i + 1);
    let name = &name_end[name_start..];
    name == "xmlns" || name.starts_with("xmlns:")
}

/// The host at the start of what follows `scheme://`.
fn host_at(rest: &str) -> Option<String> {
    let end = rest
        .find(|c: char| {
            matches!(
                c,
                '/' | '?' | '#' | '"' | '\'' | '<' | '>' | ')' | ' ' | '\n' | '\r' | '\t' | '\\'
            )
        })
        .unwrap_or(rest.len());
    let authority = &rest[..end];
    let host = authority.rsplit('@').next()?;
    let host = match host.strip_prefix('[') {
        // an IPv6 literal
        Some(v6) => v6.split(']').next()?,
        None => host.split(':').next()?,
    };
    let host = host.trim_end_matches('.');
    let ok = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'));
    ok.then(|| host.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_from_text_and_html() {
        let raw = b"Content-Type: multipart/alternative; boundary=b\r\n\r\n\
--b\r\nContent-Type: text/plain\r\n\r\nsee https://Shop.Example/x?y=1 and http://user@login.evil.example:8080/p\r\n\
--b\r\nContent-Type: text/html\r\n\r\n<a href=\"https://shop.example/a\">x</a> <a href='https://cdn.other.example'>y</a>\r\n\
--b--\r\n";
        let parsed = mailrs_mime::parse(raw);
        assert_eq!(
            link_hosts(&parsed),
            ["shop.example", "login.evil.example", "cdn.other.example"]
        );
    }

    /// Microsoft Word's HTML declares half a dozen XML namespaces on
    /// the root element, and XHTML names its DTD. Both are identifiers
    /// spelled as URLs; nothing renders them and nobody clicks them.
    #[test]
    fn a_namespace_declaration_is_not_a_link() {
        let raw = b"Content-Type: text/html\r\n\r\n\
<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\" \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd\">\r\n\
<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:o = 'urn:schemas-microsoft-com:office:office'\r\n\
 xmlns:w=\"http://schemas.microsoft.com/office/word/2003/wordml\" XMLNS:M=\"http://schemas.openxmlformats.org/officeDocument/2006/math\">\r\n\
<a href=\"https://bucket.example/x\">open</a></html>\r\n";
        assert_eq!(link_hosts(&mailrs_mime::parse(raw)), ["bucket.example"]);
    }

    /// The same hosts, used as links, are still links: what is skipped
    /// is the declaration, not the host.
    #[test]
    fn a_namespace_host_used_as_a_link_is_still_a_link() {
        let raw = b"Content-Type: text/html\r\n\r\n\
<a href=\"http://www.w3.org/1999/xhtml\">spec</a> <a data-xmlns=\"x\" href=\"https://schemas.microsoft.com/a\">b</a>\r\n\
<p>see http://schemas.openxmlformats.org/ and doctype http://doctype.example/</p>\r\n";
        assert_eq!(
            link_hosts(&mailrs_mime::parse(raw)),
            [
                "www.w3.org",
                "schemas.microsoft.com",
                "schemas.openxmlformats.org",
                "doctype.example"
            ]
        );
    }

    #[test]
    fn attachments_with_names_and_types() {
        let raw = b"Content-Type: multipart/mixed; boundary=b\r\n\r\n\
--b\r\nContent-Type: text/plain\r\n\r\nhi\r\n\
--b\r\nContent-Type: Application/PDF; name=a.pdf\r\nContent-Disposition: attachment; filename=a.pdf\r\n\r\n%PDF\r\n\
--b--\r\n";
        let parsed = mailrs_mime::parse(raw);
        let parts = attachments(&parsed);
        assert_eq!(parts.len(), 1);
        assert_eq!(
            (parts[0].name.as_str(), parts[0].declared_type.as_str()),
            ("a.pdf", "application/pdf")
        );
        assert!(parts[0].bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn odd_hosts() {
        assert_eq!(
            host_at("[2001:db8::1]:443/x").as_deref(),
            Some("2001:db8::1")
        );
        assert_eq!(host_at("").as_deref(), None);
        assert_eq!(host_at("exa mple").as_deref(), Some("exa"));
        assert_eq!(host_at("ü.example/").as_deref(), None);
    }

    #[test]
    fn junk_subjects_skip_the_readers_own_messages() {
        let theirs = b"From: Shop <deals@Shop.Example>\r\nContent-Type: text/plain\r\n\r\nsee https://trk.example/a and http://trk.example/b\r\n".to_vec();
        let again = b"From: other@shop.example\r\n\r\nhi\r\n".to_vec();
        let mine = b"From: qa@golia.jp\r\n\r\nhttps://mine.example/\r\n".to_vec();
        let got = junk_subjects(&[theirs, again, mine], "QA@golia.jp");
        assert_eq!(got.senders, ["shop.example"]);
        assert_eq!(got.links, ["trk.example"]);
        assert_eq!(junk_subjects(&[], "qa@golia.jp"), JunkSubjects::default());
    }
}
