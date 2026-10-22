//! The data items of one FETCH response (RFC 9051 §6.4.5, §7.5.2).
//!
//! The attribute list is split into items first and each item is
//! answered on its own. Matching substrings of the whole list instead
//! answered `RFC822.SIZE` with the full message too (it contains
//! `RFC822`), and never answered `BODY.PEEK[HEADER]`, `ENVELOPE` or
//! `BODY.PEEK[HEADER.FIELDS (…)]` at all.

use mailrs_imap_format::{
    build_bodystructure, build_envelope, extract_body_section, extract_header_fields,
    extract_header_fields_except, extract_header_section, extract_mime_part,
};

use super::flags::{flags_to_imap, format_internal_date};
use crate::backend::Message;

/// One requested item. `MODSEQ` is answered by the caller, which also
/// handles the `CHANGEDSINCE` modifier.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Item {
    Uid,
    Flags,
    InternalDate,
    Size,
    Envelope,
    BodyStructure,
    Modseq,
    /// `RFC822`, `RFC822.HEADER`, `RFC822.TEXT`, answered under their
    /// own name
    Rfc822(&'static str),
    /// `BODY[section]<partial>` or `BODY.PEEK[…]`
    Section {
        section: String,
        partial: Option<(usize, Option<usize>)>,
    },
}

/// The items `attrs` asks for; a UID FETCH always answers the uid.
pub(super) fn requested(attrs: &str, by_uid: bool) -> Vec<Item> {
    let mut wanted = parse_items(attrs);
    if by_uid && !wanted.contains(&Item::Uid) {
        wanted.insert(0, Item::Uid);
    }
    wanted
}

/// Whether answering `items` takes the message's bytes.
pub(super) fn needs_bytes(items: &[Item]) -> bool {
    items.iter().any(|i| {
        matches!(
            i,
            Item::Envelope | Item::BodyStructure | Item::Rfc822(_) | Item::Section { .. }
        )
    })
}

/// The answer to `items`; content items are left out when `bytes` is
/// `None` (the message is gone).
pub(super) fn answer<K>(
    msg: &Message<K>,
    items: Vec<Item>,
    bytes: Option<&[u8]>,
) -> Vec<(String, String)> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Item::Uid => out.push(("UID".into(), msg.uid.to_string())),
            Item::Flags => out.push(("FLAGS".into(), format!("({})", flags_to_imap(&msg.flags)))),
            Item::InternalDate => out.push((
                "INTERNALDATE".into(),
                format!("\"{}\"", format_internal_date(msg.internal_date)),
            )),
            Item::Size => out.push(("RFC822.SIZE".into(), msg.size.to_string())),
            Item::Modseq => {}
            Item::Envelope | Item::BodyStructure | Item::Rfc822(_) | Item::Section { .. } => {
                let Some(data) = bytes else {
                    continue;
                };
                out.push(content_item(item, data));
            }
        }
    }
    out
}

fn content_item(item: Item, data: &[u8]) -> (String, String) {
    match item {
        Item::Envelope => ("ENVELOPE".into(), build_envelope(data)),
        Item::BodyStructure => ("BODYSTRUCTURE".into(), build_bodystructure(data)),
        Item::Rfc822(name) => {
            let part = match name {
                "RFC822.HEADER" => extract_header_section(data),
                "RFC822.TEXT" => extract_body_section(data),
                _ => data.to_vec(),
            };
            (name.into(), literal(&part))
        }
        Item::Section { section, partial } => {
            let part = section_bytes(data, &section);
            let (key, part) = match partial {
                Some((start, len)) => {
                    let from = start.min(part.len());
                    let to = len.map_or(part.len(), |n| (from + n).min(part.len()));
                    (format!("BODY[{section}]<{start}>"), &part[from..to])
                }
                None => (format!("BODY[{section}]"), &part[..]),
            };
            (key, literal(part))
        }
        Item::Uid | Item::Flags | Item::InternalDate | Item::Size | Item::Modseq => {
            unreachable!("answered without the message bytes")
        }
    }
}

fn section_bytes(data: &[u8], section: &str) -> Vec<u8> {
    let upper = section.to_ascii_uppercase();
    if upper.is_empty() {
        return data.to_vec();
    }
    if upper == "HEADER" {
        return extract_header_section(data);
    }
    if upper == "TEXT" {
        return extract_body_section(data);
    }
    if let Some(rest) = upper.strip_prefix("HEADER.FIELDS.NOT") {
        return extract_header_fields_except(data, &field_names(rest));
    }
    if let Some(rest) = upper.strip_prefix("HEADER.FIELDS") {
        return extract_header_fields(data, &field_names(rest));
    }
    extract_mime_part(data, section).unwrap_or_default()
}

fn field_names(list: &str) -> Vec<String> {
    list.trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split_whitespace()
        .map(|f| f.trim_matches('"').to_string())
        .collect()
}

// the count is of the bytes sent, which is the lossy text: a message
// that is not valid UTF-8 has its invalid bytes replaced
fn literal(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    format!("{{{}}}\r\n{}", text.len(), text)
}

/// The items of a FETCH attribute list: a parenthesised list, one bare
/// item, or one of the macros `ALL`, `FAST`, `FULL`. A trailing
/// modifier list such as `(CHANGEDSINCE 42)` is not an item.
fn parse_items(attrs: &str) -> Vec<Item> {
    let attrs = attrs.trim();
    let list = match attrs.strip_prefix('(') {
        Some(rest) => &rest[..closing_paren(rest)],
        None => split_items(attrs).first().copied().unwrap_or(""),
    };
    let mut items = Vec::new();
    for token in split_items(list) {
        match token.to_ascii_uppercase().as_str() {
            "ALL" => items.extend([Item::Flags, Item::InternalDate, Item::Size, Item::Envelope]),
            "FAST" => items.extend([Item::Flags, Item::InternalDate, Item::Size]),
            "FULL" => items.extend([
                Item::Flags,
                Item::InternalDate,
                Item::Size,
                Item::Envelope,
                Item::BodyStructure,
            ]),
            "UID" => items.push(Item::Uid),
            "FLAGS" => items.push(Item::Flags),
            "INTERNALDATE" => items.push(Item::InternalDate),
            "RFC822.SIZE" => items.push(Item::Size),
            "ENVELOPE" => items.push(Item::Envelope),
            "BODYSTRUCTURE" | "BODY" => items.push(Item::BodyStructure),
            "MODSEQ" => items.push(Item::Modseq),
            "RFC822" => items.push(Item::Rfc822("RFC822")),
            "RFC822.HEADER" => items.push(Item::Rfc822("RFC822.HEADER")),
            "RFC822.TEXT" => items.push(Item::Rfc822("RFC822.TEXT")),
            upper => {
                if let Some(item) = section_item(token, upper) {
                    items.push(item);
                }
            }
        }
    }
    items
}

fn section_item(token: &str, upper: &str) -> Option<Item> {
    let prefix = if upper.starts_with("BODY.PEEK[") {
        "BODY.PEEK["
    } else if upper.starts_with("BODY[") {
        "BODY["
    } else {
        return None;
    };
    let close = token.rfind(']')?;
    let section = token[prefix.len()..close].trim().to_string();
    let partial = token[close + 1..]
        .strip_prefix('<')
        .and_then(|p| p.strip_suffix('>'))
        .and_then(|p| {
            let (start, len) = match p.split_once('.') {
                Some((s, n)) => (s, Some(n)),
                None => (p, None),
            };
            Some((start.parse().ok()?, len.and_then(|n| n.parse().ok())))
        });
    Some(Item::Section { section, partial })
}

// index of the `)` that closes a list whose `(` was already consumed
fn closing_paren(s: &str) -> usize {
    let mut depth = 0usize;
    let mut bracket = false;
    for (i, c) in s.char_indices() {
        match c {
            '[' => bracket = true,
            ']' => bracket = false,
            '(' if !bracket => depth += 1,
            ')' if !bracket => {
                if depth == 0 {
                    return i;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    s.len()
}

// spaces inside `[…]` (a header field list) do not separate items
fn split_items(list: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut bracket = false;
    let mut start = 0;
    for (i, c) in list.char_indices() {
        match c {
            '[' => bracket = true,
            ']' => bracket = false,
            ' ' if !bracket => {
                if i > start {
                    out.push(&list[start..i]);
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < list.len() {
        out.push(&list[start..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &str = "From: Ann <ann@a.jp>\r\nTo: b@b.jp\r\nSubject: hello world\r\nX-Other: 1\r\n\r\nthe body\r\n";

    fn message() -> Message<()> {
        Message {
            uid: 7,
            seqno: 1,
            flags: vec![],
            internal_date: 0,
            size: RAW.len() as u64,
            modseq: 1,
            key: (),
        }
    }

    fn fetch_by(attrs: &str, by_uid: bool) -> Vec<(String, String)> {
        answer(&message(), requested(attrs, by_uid), Some(RAW.as_bytes()))
    }

    fn fetch(attrs: &str) -> Vec<(String, String)> {
        fetch_by(attrs, false)
    }

    fn keys(items: &[(String, String)]) -> Vec<&str> {
        items.iter().map(|(k, _)| k.as_str()).collect()
    }

    #[test]
    fn envelope_is_answered() {
        let items = fetch("(UID ENVELOPE)");
        assert_eq!(keys(&items), ["UID", "ENVELOPE"]);
        assert!(
            items[1]
                .1
                .starts_with(r#"(NIL "hello world" (("Ann" NIL "ann" "a.jp"))"#)
        );
    }

    #[test]
    fn peek_header_fields_answers_only_those_fields() {
        let items = fetch("(UID BODY.PEEK[HEADER.FIELDS (SUBJECT FROM)])");
        assert_eq!(keys(&items), ["UID", "BODY[HEADER.FIELDS (SUBJECT FROM)]"]);
        let body = "From: Ann <ann@a.jp>\r\nSubject: hello world\r\n\r\n";
        assert_eq!(items[1].1, format!("{{{}}}\r\n{body}", body.len()));
    }

    #[test]
    fn header_fields_not_leaves_the_named_fields_out() {
        let items = fetch("BODY.PEEK[HEADER.FIELDS.NOT (X-OTHER TO)]");
        assert!(
            items[0]
                .1
                .ends_with("From: Ann <ann@a.jp>\r\nSubject: hello world\r\n\r\n")
        );
    }

    #[test]
    fn peek_header_and_text_are_answered_without_peek_in_the_name() {
        let items = fetch("(BODY.PEEK[HEADER] BODY.PEEK[TEXT])");
        assert_eq!(keys(&items), ["BODY[HEADER]", "BODY[TEXT]"]);
        assert!(items[1].1.ends_with("\r\nthe body\r\n"));
    }

    #[test]
    fn rfc822_size_does_not_send_the_message() {
        let items = requested("(RFC822.SIZE)", false);
        assert!(!needs_bytes(&items));
        let items = answer(&message(), items, None);
        assert_eq!(items, [("RFC822.SIZE".to_string(), RAW.len().to_string())]);
    }

    #[test]
    fn a_partial_fetch_sends_the_slice_and_names_its_origin() {
        let items = fetch("BODY.PEEK[]<0.4>");
        assert_eq!(
            items,
            [("BODY[]<0>".to_string(), "{4}\r\nFrom".to_string())]
        );
    }

    #[test]
    fn macros_expand_and_the_changedsince_modifier_is_not_an_item() {
        assert_eq!(
            keys(&fetch("ALL")),
            ["FLAGS", "INTERNALDATE", "RFC822.SIZE", "ENVELOPE"]
        );
        assert_eq!(keys(&fetch("(FLAGS) (CHANGEDSINCE 42)")), ["FLAGS"]);
    }

    #[test]
    fn uid_fetch_always_carries_the_uid() {
        let items = fetch_by("(FLAGS)", true);
        assert_eq!(keys(&items), ["UID", "FLAGS"]);
    }
}
