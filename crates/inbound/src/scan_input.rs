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
        let mut from = 0;
        while let Some(at) = lower[from..].find("http") {
            let after = from + at + 4;
            let skip = match &lower[after..] {
                r if r.starts_with("://") => Some(3),
                r if r.starts_with("s://") => Some(4),
                _ => None,
            };
            from = after;
            let Some(skip) = skip else {
                continue;
            };
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
