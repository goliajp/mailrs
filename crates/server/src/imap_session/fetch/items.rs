//! What a FETCH asked for, and the response items that need no message body.

use mailrs_imap_format::{
    format_addr_list, format_imap_flags, format_internal_date, parse_generic_body_sections,
    parse_header_fields_request, quote_or_nil,
};
use mailrs_mailbox::MessageMeta;

/// The attribute list of one FETCH command, parsed once for every message.
pub(super) struct FetchAttrs {
    pub(super) flags: bool,
    pub(super) uid: bool,
    pub(super) rfc822_size: bool,
    pub(super) internaldate: bool,
    pub(super) envelope: bool,
    pub(super) body_peek: bool,
    pub(super) body_full: bool,
    pub(super) body_header: bool,
    pub(super) body_text: bool,
    pub(super) bodystructure: bool,
    pub(super) modseq: bool,
    pub(super) header_fields_request: Option<(Vec<String>, String)>,
    pub(super) generic_body_sections: Vec<String>,
    pub(super) any_body: bool,
    pub(super) changedsince: Option<u64>,
    /// Whether the attribute list says PEEK anywhere: a generic section
    /// fetched without it marks the message seen.
    pub(super) peek: bool,
}

impl FetchAttrs {
    pub(super) fn parse(attributes: &str, use_uid: bool) -> Self {
        // parse requested attributes. IMAP attribute names are ASCII
        // per RFC 9051 §6.4.5; ASCII fold avoids walking the Unicode
        // case-fold table for every FETCH command.
        let attrs_upper = attributes.to_ascii_uppercase();
        let want_flags = attrs_upper.contains("FLAGS");
        let want_uid = attrs_upper.contains("UID") || use_uid;
        let want_rfc822_size = attrs_upper.contains("RFC822.SIZE");
        let want_internaldate = attrs_upper.contains("INTERNALDATE");
        let want_envelope = attrs_upper.contains("ENVELOPE");
        let want_body_peek = attrs_upper.contains("BODY.PEEK[]");
        // check for standalone RFC822 (not RFC822.SIZE, RFC822.HEADER, RFC822.TEXT)
        let has_standalone_rfc822 = attrs_upper
            .split_whitespace()
            .any(|w| w == "RFC822" || w == "(RFC822" || w == "RFC822)");
        let want_body_full =
            !want_body_peek && (attrs_upper.contains("BODY[]") || has_standalone_rfc822);
        let want_body_header = attrs_upper.contains("BODY[HEADER]")
            || attrs_upper.contains("BODY.PEEK[HEADER]")
            || attrs_upper.contains("RFC822.HEADER");
        let want_body_text = attrs_upper.contains("BODY[TEXT]")
            || attrs_upper.contains("BODY.PEEK[TEXT]")
            || attrs_upper.contains("RFC822.TEXT");
        let want_bodystructure = attrs_upper.contains("BODYSTRUCTURE");
        let want_modseq = attrs_upper.contains("MODSEQ");

        // BODY[HEADER.FIELDS (field-list)] / BODY.PEEK[HEADER.FIELDS (field-list)]
        let header_fields_request = parse_header_fields_request(attributes);

        // generic BODY[section] requests (e.g. BODY[1], BODY[1.1], BODY[1.MIME])
        let generic_body_sections = parse_generic_body_sections(attributes);

        let want_any_body = want_body_peek
            || want_body_full
            || want_body_header
            || want_body_text
            || header_fields_request.is_some()
            || !generic_body_sections.is_empty();

        Self {
            flags: want_flags,
            uid: want_uid,
            rfc822_size: want_rfc822_size,
            internaldate: want_internaldate,
            envelope: want_envelope,
            body_peek: want_body_peek,
            body_full: want_body_full,
            body_header: want_body_header,
            body_text: want_body_text,
            bodystructure: want_bodystructure,
            modseq: want_modseq,
            header_fields_request,
            generic_body_sections,
            any_body: want_any_body,
            changedsince: changedsince(attributes, &attrs_upper),
            peek: attrs_upper.contains("PEEK"),
        }
    }
}

/// CHANGEDSINCE modifier (RFC 7162)
fn changedsince(attributes: &str, attrs_upper: &str) -> Option<u64> {
    if let Some(pos) = attrs_upper.find("CHANGEDSINCE") {
        let after = attributes.get(pos + "CHANGEDSINCE".len()..).unwrap_or("");
        let after = after.trim_start();
        after.split_whitespace().next().and_then(|s| {
            // strip trailing parenthesis if present
            s.trim_end_matches(')').parse::<u64>().ok()
        })
    } else {
        None
    }
}

/// The 1-based sequence number of `msg`, or `None` when the request's set
/// does not include it.
pub(super) fn requested_sequence_number(
    messages: &[MessageMeta],
    msg: &MessageMeta,
    uids: &[u32],
    use_uid: bool,
) -> Option<u32> {
    let seq_num = if use_uid {
        // check if this UID is in the requested set
        if !uids.contains(&msg.uid) {
            return None;
        }
        // find sequence number (1-based position in the list)
        messages
            .iter()
            .position(|m| m.uid == msg.uid)
            .map(|p| p as u32 + 1)
            .unwrap_or(0)
    } else {
        let seq = messages
            .iter()
            .position(|m| m.uid == msg.uid)
            .map(|p| p as u32 + 1)
            .unwrap_or(0);
        if !uids.contains(&seq) {
            return None;
        }
        seq
    };

    if seq_num == 0 {
        return None;
    }
    Some(seq_num)
}

/// FLAGS, UID, RFC822.SIZE, INTERNALDATE, ENVELOPE and MODSEQ: the items
/// answered from the index row alone.
pub(super) fn metadata_items(msg: &MessageMeta, attrs: &FetchAttrs) -> Vec<Vec<u8>> {
    // items are built as Vec<u8> to handle binary literal data correctly
    let mut items: Vec<Vec<u8>> = Vec::new();
    if attrs.flags {
        items.push(format!("FLAGS ({})", format_imap_flags(msg.flags)).into_bytes());
    }
    if attrs.uid {
        items.push(format!("UID {}", msg.uid).into_bytes());
    }
    if attrs.rfc822_size {
        items.push(format!("RFC822.SIZE {}", msg.size).into_bytes());
    }
    if attrs.internaldate {
        items.push(
            format!(
                "INTERNALDATE \"{}\"",
                format_internal_date(msg.internal_date)
            )
            .into_bytes(),
        );
    }
    if attrs.envelope {
        let date = format_internal_date(msg.internal_date);
        let from = format_addr_list(&msg.sender);
        let to = format_addr_list(&msg.recipients);
        items.push(
            format!(
                "ENVELOPE ({} {} {} {} {} {} NIL NIL {} {})",
                quote_or_nil(&date),
                quote_or_nil(&msg.subject),
                from,
                from,
                from,
                to,
                quote_or_nil(&msg.in_reply_to),
                quote_or_nil(&msg.message_id),
            )
            .into_bytes(),
        );
    }

    if attrs.modseq || attrs.changedsince.is_some() {
        items.push(format!("MODSEQ ({})", msg.modseq).into_bytes());
    }
    items
}

/// One literal item: `<name> {<len>}\r\n` followed by the raw bytes.
pub(super) fn literal_item(name: &str, data: &[u8]) -> Vec<u8> {
    let mut item = format!("{name} {{{}}}\r\n", data.len()).into_bytes();
    item.extend_from_slice(data);
    item
}

/// The full FETCH response line for one message, as bytes.
pub(super) fn fetch_response_line(seq_num: u32, items: &[Vec<u8>]) -> Vec<u8> {
    let mut resp = format!("* {} FETCH (", seq_num).into_bytes();
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            resp.push(b' ');
        }
        resp.extend_from_slice(item);
    }
    resp.extend_from_slice(b")\r\n");
    resp
}
