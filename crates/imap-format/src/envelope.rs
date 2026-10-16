//! The FETCH `ENVELOPE` structure (RFC 9051 §7.5.2), built from a
//! message's own header block.
//!
//! Field values are sent as they appear on the wire, encoded words
//! included: decoding them is the client's job, as the RFC specifies.

use mailrs_rfc5322::{Message, split_list};

use crate::{escape_imap_string, quote_or_nil};

/// `(date subject from sender reply-to to cc bcc in-reply-to message-id)`
/// for the raw message `data`.
///
/// `Sender` and `Reply-To` default to `From` when absent, as the RFC
/// requires; any other missing field is `NIL`.
///
/// ```
/// use mailrs_imap_format::build_envelope;
/// let raw = b"From: Ann <ann@a.jp>\r\nTo: b@b.jp\r\nSubject: hi there\r\n\r\nbody";
/// let env = build_envelope(raw);
/// assert!(env.starts_with("(NIL \"hi there\" ((\"Ann\" NIL \"ann\" \"a.jp\"))"));
/// ```
pub fn build_envelope(data: &[u8]) -> String {
    let msg = Message::new(data);
    let field = |name: &str| msg.header(name).map(unfold).unwrap_or_default();
    let from = address_list(&field("From"));
    let sender = or_from(address_list(&field("Sender")), &from);
    let reply_to = or_from(address_list(&field("Reply-To")), &from);
    format!(
        "({} {} {from} {sender} {reply_to} {} {} {} {} {})",
        quote_or_nil(&field("Date")),
        quote_or_nil(&field("Subject")),
        address_list(&field("To")),
        address_list(&field("Cc")),
        address_list(&field("Bcc")),
        quote_or_nil(&field("In-Reply-To")),
        quote_or_nil(&field("Message-ID")),
    )
}

fn or_from(list: String, from: &str) -> String {
    if list == "NIL" {
        from.to_string()
    } else {
        list
    }
}

// a folded header is one logical line; CR and LF cannot appear in a
// quoted string
fn unfold(value: &[u8]) -> String {
    let text = String::from_utf8_lossy(value);
    let mut out = String::with_capacity(text.len());
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        if out.is_empty() {
            out.push_str(line.trim());
        } else {
            let line = line.trim_start();
            if !line.is_empty() {
                out.push(' ');
                out.push_str(line);
            }
        }
    }
    out
}

// commas inside a quoted display name do not separate addresses; an
// empty group such as `undisclosed-recipients:;` names no mailbox
fn address_list(list: &str) -> String {
    let parts: String = split_list(list)
        .filter(|item| !item.ends_with(';') || item.contains('@'))
        .map(address)
        .collect();
    if parts.is_empty() {
        "NIL".to_string()
    } else {
        format!("({parts})")
    }
}

fn address(item: &str) -> String {
    let (name, spec) = match (item.rfind('<'), item.rfind('>')) {
        (Some(open), Some(close)) if close > open => (
            item[..open].trim().trim_matches('"'),
            &item[open + 1..close],
        ),
        _ => ("", item),
    };
    let (mailbox, host) = spec.trim().rsplit_once('@').unwrap_or((spec.trim(), ""));
    format!(
        "({} NIL \"{}\" \"{}\")",
        quote_or_nil(name),
        escape_imap_string(mailbox),
        escape_imap_string(host)
    )
}

#[cfg(test)]
mod tests {
    use super::build_envelope;

    #[test]
    fn every_field_in_rfc_order() {
        let raw = b"Date: Fri, 16 Oct 2026 10:00:00 +0900\r\n\
From: \"Doe, Jane\" <jane@a.jp>\r\n\
To: b@b.jp, Carl <c@c.jp>\r\n\
Cc: d@d.jp\r\n\
Subject: quarterly report\r\n\
In-Reply-To: <x@a.jp>\r\n\
Message-ID: <y@a.jp>\r\n\r\nbody";
        let jane = r#"(("Doe, Jane" NIL "jane" "a.jp"))"#;
        assert_eq!(
            build_envelope(raw),
            format!(
                r#"("Fri, 16 Oct 2026 10:00:00 +0900" "quarterly report" {jane} {jane} {jane} ((NIL NIL "b" "b.jp")("Carl" NIL "c" "c.jp")) ((NIL NIL "d" "d.jp")) NIL "<x@a.jp>" "<y@a.jp>")"#
            )
        );
    }

    #[test]
    fn sender_and_reply_to_stand_apart_from_from_when_present() {
        let raw = b"From: a@a.jp\r\nSender: s@a.jp\r\nReply-To: r@a.jp\r\n\r\n";
        let env = build_envelope(raw);
        assert!(
            env.contains(r#"((NIL NIL "a" "a.jp")) ((NIL NIL "s" "a.jp")) ((NIL NIL "r" "a.jp"))"#)
        );
    }

    #[test]
    fn a_folded_subject_is_one_line_and_quotes_are_escaped() {
        let raw = b"Subject: a \"long\"\r\n  subject\r\n\r\n";
        assert!(build_envelope(raw).starts_with(r#"(NIL "a \"long\" subject" "#));
    }

    #[test]
    fn a_message_without_headers_is_all_nil() {
        assert_eq!(
            build_envelope(b"\r\nbody"),
            "(NIL NIL NIL NIL NIL NIL NIL NIL NIL NIL)"
        );
    }

    #[test]
    fn an_empty_group_names_no_one() {
        let raw = b"To: undisclosed-recipients:;\r\n\r\n";
        assert!(build_envelope(raw).contains(" NIL NIL NIL NIL NIL NIL)"));
    }
}
