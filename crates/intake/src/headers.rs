//! Reading a message far enough to file it: headers, the sender-auth
//! verdict, the sort date, the id a message without one goes by, and the
//! text the list preview and search read.

/// A Message-ID or address header value without its angle brackets.
pub fn strip_angle(v: &str) -> String {
    let t = v.trim();
    if let Some(inner) = t.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
        inner.trim().to_string()
    } else {
        t.trim_matches(|c: char| c == '<' || c == '>').to_string()
    }
}

/// An RFC 5322 `Date:` value as unix seconds, zone applied. `None` when
/// empty or unparseable.
pub fn parse_rfc5322_date(s: &str) -> Option<i64> {
    mailrs_inbound::dates::parse_date(s)
}

/// Extract common headers from an RFC 5322 message. Returns
/// `(message_id, in_reply_to, references, subject, date_epoch, from, to)`.
///
/// `references` is every Message-ID token of the References header,
/// oldest (root) first. Threading resolves against the msgid→thread
/// index via `resolve_thread_by_ancestry`; `references[0]` is only the
/// last-resort root guess (it is NOT stable across hops — remote MUAs
/// rewrite it, which fragmented conversations before v2.9.5).
/// The sender verdict for one stored message, folded to a stable token.
/// The self-hosted "is this sender who they claim to be" signal — no
/// model involved.
///
/// Two inputs, and the order between them matters. The message's own
/// `Authentication-Results` header answers *did this come from the
/// domain the From header names*; the From display name and Subject
/// answer *was that header arranged to render as somebody else*. The
/// second outranks the first, because an attacker on a domain they own
/// passes the first for free.
///
/// Empty means **nothing to say**: no authentication header and no
/// tampering found. It never means the message was examined and found
/// safe.
pub fn extract_sender_trust(raw: &[u8]) -> String {
    let head = &raw[..raw.len().min(16 * 1024)];
    // Find the (possibly folded) Authentication-Results field. Headers
    // are ASCII field names; scan lines, unfolding continuations.
    let text = String::from_utf8_lossy(head);
    let mut value: Option<String> = None;
    let mut collecting = false;
    for line in text.split("\r\n").flat_map(|l| l.split('\n')) {
        if collecting {
            if line.starts_with(' ') || line.starts_with('\t') {
                value.as_mut().unwrap().push(' ');
                value.as_mut().unwrap().push_str(line.trim());
                continue;
            }
            break; // header ended
        }
        if let Some(rest) = line
            .strip_prefix("Authentication-Results:")
            .or_else(|| line.strip_prefix("authentication-results:"))
        {
            value = Some(rest.trim().to_string());
            collecting = true;
        }
    }
    // What the From display name and Subject actually contain. This is
    // read before the authentication results are consulted, and it
    // outranks them, because the two answer different questions: DMARC
    // says the mail came from the domain in the From header, and a
    // right-to-left override says somebody arranged for that header to
    // render as a different name. An attacker on their own domain gets
    // the first for free, which is how a JCB phish earned a green badge
    // on 2026-08-16.
    let deception = mailrs_inbound::deception_in_identity(raw);
    let Some(v) = value else {
        // No verdict to fold — but an override is a property of the
        // text, not of the authentication, so it still convicts.
        return if deception.bidi_override {
            mailrs_inbound::SenderTrust::Suspicious.as_str().to_string()
        } else {
            String::new()
        };
    };
    let results = mailrs_inbound::parse_auth_results(&v);
    if results.is_empty() {
        return if deception.bidi_override {
            mailrs_inbound::SenderTrust::Suspicious.as_str().to_string()
        } else {
            String::new()
        };
    }
    mailrs_inbound::sender_trust_with(&results, deception)
        .as_str()
        .to_string()
}

pub fn extract_headers(raw: &[u8]) -> (String, String, Vec<String>, String, i64, String, String) {
    let mut message_id = String::new();
    let mut in_reply_to = String::new();
    let mut references: Vec<String> = Vec::new();
    let mut subject = String::new();
    let mut date_epoch: i64 = 0;
    let mut from = String::new();
    let mut to = String::new();

    // We only need headers; stop at the first blank line.
    let head_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n"))
        .unwrap_or(raw.len());
    let head = &raw[..head_end];
    let s = String::from_utf8_lossy(head);
    // Unfold headers (RFC 5322 §2.2.3 — a header continues onto the
    // next line if that line starts with WSP).
    let mut cur = String::new();
    let mut lines: Vec<String> = Vec::new();
    for line in s.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.starts_with(' ') || line.starts_with('\t') {
            cur.push(' ');
            cur.push_str(line.trim_start());
        } else {
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            cur.push_str(line);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    for l in &lines {
        let Some((name, val)) = l.split_once(':') else {
            continue;
        };
        let val = val.trim();
        match name.to_ascii_lowercase().as_str() {
            "message-id" => message_id = strip_angle(val),
            "in-reply-to" => in_reply_to = strip_angle(val),
            "references" => {
                // Every <...> token, oldest (root) first — the full chain
                // feeds the msgid→thread resolver, not just token 0.
                references = val
                    .split_whitespace()
                    .filter_map(|tok| {
                        let t = tok.trim_matches(|c: char| c == '<' || c == '>' || c == ',');
                        (!t.is_empty()).then(|| t.to_string())
                    })
                    .collect();
            }
            "subject" => subject = mailrs_rfc2047::decode(val.as_bytes()).into_owned(),
            // display-name part of address headers is rfc2047-encoded by
            // many senders — decode here so stores never hold =?..?= runes
            "from" => from = mailrs_rfc2047::decode(val.as_bytes()).into_owned(),
            "to" => to = mailrs_rfc2047::decode(val.as_bytes()).into_owned(),
            "date" => date_epoch = parse_rfc5322_date(val).unwrap_or(0),
            _ => {}
        }
    }
    (
        message_id,
        in_reply_to,
        references,
        subject,
        date_epoch,
        from,
        to,
    )
}

/// Extract the delivery epoch from a Maildir filename. The Maildir
/// naming convention (`<epoch>.M<micro>P<pid>Q<seq>.<host>`) records
/// the delivery second in the leading component — a reliable fallback
/// when the message's `Date:` header is missing or unparseable. Filter
/// out obviously bogus epochs (<= year 2000) so we don't backdate
/// modern mail into 1970 territory.
/// The time a message sorts by: its `Date:`, unless that is missing or
/// later than its arrival. A sender who writes a date a month ahead
/// would otherwise keep the conversation above everything that arrives
/// until then.
pub fn sort_date(header: i64, arrival: Option<i64>) -> Option<i64> {
    match arrival {
        Some(arrival) if header <= 0 || header > arrival => Some(arrival),
        _ if header > 0 => Some(header),
        _ => None,
    }
}

pub fn maildir_filename_epoch(name: &str) -> Option<i64> {
    let first = name.split('.').next()?;
    let n: i64 = first.parse().ok()?;
    if n > 946_684_800 { Some(n) } else { None }
}

/// `header_id` when the message has one, otherwise an id derived from its
/// maildir file name: `<epoch>.<unique>@mailrs.local`.
///
/// Every path that reads the file (delivery, the self-heal sweep, the file
/// index) must arrive at the same id, so it comes from the name and not
/// from the clock, and the `:2,` flags are left off: they change when the
/// message is read, the id must not. The form is the one the monolith
/// minted at delivery, so mail it indexed is found again, not indexed a
/// second time.
pub fn message_id_or_file_id(header_id: String, blob_ref: &str) -> String {
    if !header_id.is_empty() {
        return header_id;
    }
    let name = blob_ref.rsplit('/').next().unwrap_or(blob_ref);
    let unique = name.split_once(":2,").map_or(name, |(u, _)| u);
    match maildir_filename_epoch(unique) {
        Some(epoch) => format!("{epoch}.{unique}@mailrs.local"),
        None => format!("{unique}@mailrs.local"),
    }
}

/// Extract the searchable text of a message: the `text/plain` part if
/// there is one, else the `text/html` part flattened. Returns `None`
/// when neither exists (a bare attachment, say) so the caller can skip
/// writing an empty row.
pub fn body_text_for_search(root: &mailrs_mime::Part<'_>) -> Option<String> {
    let mut html: Option<String> = None;
    for part in root.walk() {
        match part.content_type.mime_type().as_str() {
            "text/plain" => {
                if let Some(t) = part.body_text() {
                    return Some(t);
                }
            }
            "text/html" if html.is_none() => html = part.body_text(),
            _ => {}
        }
    }
    if let Some(h) = html {
        return Some(html2text::from_read(h.as_bytes(), 100).unwrap_or(h));
    }
    root.body_text()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_ahead_of_arrival_sorts_at_arrival() {
        // tintgate.com, delivered 2026-10-12, dated 2026-11-12
        assert_eq!(
            sort_date(1_794_508_832, Some(1_791_830_446)),
            Some(1_791_830_446)
        );
    }

    #[test]
    fn a_date_before_arrival_is_kept() {
        assert_eq!(
            sort_date(1_791_830_000, Some(1_791_830_446)),
            Some(1_791_830_000)
        );
        assert_eq!(sort_date(1_791_830_000, None), Some(1_791_830_000));
    }

    #[test]
    fn no_date_falls_back_to_arrival() {
        assert_eq!(sort_date(0, Some(1_791_830_446)), Some(1_791_830_446));
        assert_eq!(sort_date(0, None), None);
    }

    /// A From display name encoded per RFC 2047, the way the real ones
    /// arrive — the override is invisible in the raw bytes.
    fn message_from(display_name: &str, auth: Option<&str>) -> Vec<u8> {
        let encoded = mailrs_rfc2047::encode(display_name);
        let auth_line = match auth {
            Some(a) => format!("Authentication-Results: mail.golia.ai; {a}\r\n"),
            None => String::new(),
        };
        format!(
            "{auth_line}From: {encoded} <alertpq43@wokjx.crabfishhh.com>\r\n\
             Subject: test\r\n\r\nbody\r\n"
        )
        .into_bytes()
    }

    /// **The reported message.** Every check passed and every check was
    /// right — the attacker owns `wokjx.crabfishhh.com`, and
    /// authentication records are free on a domain you control. The lie
    /// was `MyJCB` written backwards behind an override, which is not
    /// something DMARC has an opinion about.
    #[test]
    fn a_passing_dmarc_does_not_outrank_a_reversed_display_name() {
        let raw = message_from("\u{202E}BCJyM", Some("spf=pass; dkim=pass; dmarc=pass"));
        assert_eq!(
            extract_sender_trust(&raw),
            "suspicious",
            "a right-to-left override in the display name was outvoted by DMARC"
        );
    }

    /// The same authentication results on an untampered name must still
    /// read as verified — otherwise this is not a discriminating check,
    /// it is a verdict suppressed for everybody.
    #[test]
    fn the_same_checks_on_an_untampered_name_still_verify() {
        let raw = message_from("MyJCB", Some("spf=pass; dkim=pass; dmarc=pass"));
        assert_eq!(extract_sender_trust(&raw), "verified");
    }

    /// Mail that reached the maildir by a path that never stamped an
    /// `Authentication-Results` header has no verdict to fold. An
    /// override is a fact about the text rather than about the
    /// authentication, so it convicts anyway.
    #[test]
    fn an_override_convicts_even_with_no_authentication_header() {
        let raw = message_from("\u{202E}DRAC NOSIAS", None);
        assert_eq!(
            extract_sender_trust(&raw),
            "suspicious",
            "with no auth header the display name was never looked at"
        );
    }

    /// And ordinary mail with no auth header still reports nothing,
    /// rather than acquiring a verdict from this change.
    #[test]
    fn an_ordinary_message_without_an_auth_header_stays_silent() {
        assert_eq!(
            extract_sender_trust(&message_from("Quora Digest", None)),
            ""
        );
    }

    /// Zero-width padding is the weaker signal — one production message
    /// in forty carrying it was a real newsletter. It feeds the spam
    /// score at receive time and must not become a verdict here.
    #[test]
    fn zero_width_padding_alone_is_not_a_verdict() {
        let raw = message_from(
            "M\u{200B}yJC\u{2060}B",
            Some("spf=pass; dkim=pass; dmarc=pass"),
        );
        assert_eq!(extract_sender_trust(&raw), "verified");
    }
}

#[cfg(test)]
mod file_id_tests {
    use super::*;

    #[test]
    fn a_header_id_is_kept() {
        assert_eq!(
            message_id_or_file_id("a@b".into(), "1790000000.M1P1.h"),
            "a@b"
        );
    }

    /// The monolith's form for the same file, whatever folder or flags the
    /// file has since gained.
    #[test]
    fn a_missing_id_comes_from_the_file_name_and_survives_reading() {
        let want = "1781066005.1781066005.M390270P1Q0.8f4314585e63@mailrs.local";
        for name in [
            "1781066005.M390270P1Q0.8f4314585e63",
            "1781066005.M390270P1Q0.8f4314585e63:2,S",
            ".Archive/1781066005.M390270P1Q0.8f4314585e63:2,RS",
        ] {
            assert_eq!(message_id_or_file_id(String::new(), name), want, "{name}");
        }
        assert_eq!(
            message_id_or_file_id(String::new(), "odd-name"),
            "odd-name@mailrs.local"
        );
    }
}
