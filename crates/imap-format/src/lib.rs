//! IMAP wire-format helpers (RFC 9051 §6.4 FETCH responses, §7.5
//! BODYSTRUCTURE assembly, §9 ABNF for FLAGS / INTERNALDATE).
//!
//! Pairs with [`mailrs-imap-proto`](https://crates.io/crates/mailrs-imap-proto)
//! (command parsing + session state machine) and
//! [`mailrs-imap-codec`](https://crates.io/crates/mailrs-imap-codec)
//! (line / literal framing) to form a complete RFC 9051 receive +
//! response stack.
//!
//! 22 standalone helpers grouped by concern:
//!
//! - **FLAGS** — `format_imap_flags` / `parse_imap_flags` map the 6
//!   standard system flags between `u32` bitmask and IMAP
//!   `(\Seen \Flagged …)` syntax. Bit assignments are exposed via
//!   the `FLAG_*` `pub const` so callers can construct masks
//!   without round-tripping through strings.
//! - **INTERNALDATE** — `format_internal_date(i64)` formats a Unix
//!   timestamp as IMAP's `"DD-Mon-YYYY HH:MM:SS +ZZZZ"`.
//! - **String quoting** — `escape_imap_string` / `escape_imap_str`
//!   / `quote_or_nil` handle IMAP's `"…"` quoted-string with
//!   `\`-escapes-or-`NIL` rules.
//! - **Address parsing** — `format_imap_address` + `format_addr_list`
//!   turn RFC 5322 addresses (with optional display name) into
//!   IMAP's `((name route mailbox host))` structure.
//! - **BODY[] section requests** — `parse_header_fields_request`
//!   (`BODY[HEADER.FIELDS (…)]`) + `parse_generic_body_sections`
//!   (`BODY[1]`, `BODY[1.MIME]`, etc.).
//! - **MIME walk** — `extract_header_section` / `extract_body_section`
//!   / `extract_header_fields` / `parse_mime_headers` (returns
//!   [`MimeInfo`]) / `split_mime_parts` / `find_line_offset` /
//!   `trim_part_trailing_newline` / `extract_mime_part`. Lives in
//!   the [`mime`] module.
//! - **BODYSTRUCTURE** — `build_bodystructure` recurses through
//!   multipart trees and emits the RFC-9051 §7.5.2 form. Lives in
//!   the [`bodystructure`] module.
//!
//! All helpers are pure functions — no I/O, no async.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod bodystructure;
pub mod mime;

pub use bodystructure::build_bodystructure;
pub use mime::{
    MimeInfo, extract_body_section, extract_header_fields, extract_header_section,
    extract_mime_part, find_line_offset, parse_mime_headers, split_mime_parts,
    trim_part_trailing_newline,
};

/// IMAP `\Seen` system flag. Bit-0 of the standard u32 mask.
pub const FLAG_SEEN: u32 = 0b0000_0001;
/// IMAP `\Answered` system flag. Bit-1.
pub const FLAG_ANSWERED: u32 = 0b0000_0010;
/// IMAP `\Flagged` system flag (star). Bit-2.
pub const FLAG_FLAGGED: u32 = 0b0000_0100;
/// IMAP `\Deleted` system flag (queued for expunge). Bit-3.
pub const FLAG_DELETED: u32 = 0b0000_1000;
/// IMAP `\Draft` system flag. Bit-4.
pub const FLAG_DRAFT: u32 = 0b0001_0000;
/// IMAP `\Recent` system flag (set by EXAMINE/SELECT). Bit-5.
pub const FLAG_RECENT: u32 = 0b0010_0000;

/// convert bitmask flags to IMAP flag string
pub fn format_imap_flags(flags: u32) -> String {
    // Per-bit lookup table — `(bit_const, "\\Name")`. Layout sized to
    // the worst-case all-six-flags output ("\\Seen \\Answered \\Flagged
    // \\Deleted \\Draft \\Recent" = 47 chars) so the String never has
    // to reallocate on push_str.
    const ENTRIES: [(u32, &str); 6] = [
        (FLAG_SEEN, "\\Seen"),
        (FLAG_ANSWERED, "\\Answered"),
        (FLAG_FLAGGED, "\\Flagged"),
        (FLAG_DELETED, "\\Deleted"),
        (FLAG_DRAFT, "\\Draft"),
        (FLAG_RECENT, "\\Recent"),
    ];
    let mut out = String::with_capacity(47);
    let mut first = true;
    for (bit, name) in ENTRIES {
        if flags & bit != 0 {
            if !first {
                out.push(' ');
            }
            out.push_str(name);
            first = false;
        }
    }
    out
}

/// parse IMAP flag names from a FLAGS string like "(\\Seen \\Flagged)"
pub fn parse_imap_flags(s: &str) -> u32 {
    let s = s.trim().trim_start_matches('(').trim_end_matches(')');
    let mut bits = 0u32;
    for part in s.split_whitespace() {
        let flag = part.trim_start_matches('\\').as_bytes();
        // ASCII-case-insensitive comparison against the 6 system flag
        // names. Avoids the `.to_uppercase()` allocation that
        // dominated the hot path (~25 ns / call when allocating an
        // owned String for the match arm).
        let matched = match flag.len() {
            4 => match_4_byte_ci(flag, b"SEEN", FLAG_SEEN),
            5 => match_5_byte_ci(flag, b"DRAFT", FLAG_DRAFT),
            6 => match_6_byte_ci(flag, b"RECENT", FLAG_RECENT),
            7 => match_7_byte_ci(flag, b"DELETED", FLAG_DELETED)
                .or_else(|| match_7_byte_ci(flag, b"FLAGGED", FLAG_FLAGGED)),
            8 => match_8_byte_ci(flag, b"ANSWERED", FLAG_ANSWERED),
            _ => None,
        };
        if let Some(bit) = matched {
            bits |= bit;
        }
    }
    bits
}

// Length-keyed ASCII case-insensitive matchers. Inlined into the
// `match` so LLVM can unroll the 4/5/6/7/8-byte path it takes.
#[inline(always)]
fn match_4_byte_ci(input: &[u8], target: &[u8; 4], bit: u32) -> Option<u32> {
    debug_assert_eq!(input.len(), 4);
    if input[0].eq_ignore_ascii_case(&target[0])
        && input[1].eq_ignore_ascii_case(&target[1])
        && input[2].eq_ignore_ascii_case(&target[2])
        && input[3].eq_ignore_ascii_case(&target[3])
    {
        Some(bit)
    } else {
        None
    }
}
#[inline(always)]
fn match_5_byte_ci(input: &[u8], target: &[u8; 5], bit: u32) -> Option<u32> {
    if input.eq_ignore_ascii_case(target) {
        Some(bit)
    } else {
        None
    }
}
#[inline(always)]
fn match_6_byte_ci(input: &[u8], target: &[u8; 6], bit: u32) -> Option<u32> {
    if input.eq_ignore_ascii_case(target) {
        Some(bit)
    } else {
        None
    }
}
#[inline(always)]
fn match_7_byte_ci(input: &[u8], target: &[u8; 7], bit: u32) -> Option<u32> {
    if input.eq_ignore_ascii_case(target) {
        Some(bit)
    } else {
        None
    }
}
#[inline(always)]
fn match_8_byte_ci(input: &[u8], target: &[u8; 8], bit: u32) -> Option<u32> {
    if input.eq_ignore_ascii_case(target) {
        Some(bit)
    } else {
        None
    }
}

/// Format a Unix timestamp as an IMAP `INTERNALDATE` value
/// (`"DD-Mon-YYYY HH:MM:SS +ZZZZ"`, RFC 9051 §9 ABNF
/// `date-time`).
pub fn format_internal_date(timestamp: i64) -> String {
    use chrono::DateTime;
    let dt = DateTime::from_timestamp(timestamp, 0).unwrap_or_default();
    dt.format("%d-%b-%Y %H:%M:%S %z").to_string()
}

/// Escape `\` and `"` characters for embedding inside an IMAP
/// `"…"` quoted string. Does NOT add the surrounding quotes —
/// see [`quote_or_nil`] for the full quoted-or-`NIL` decision.
pub fn escape_imap_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// quote a string for IMAP or return NIL if empty
pub fn quote_or_nil(s: &str) -> String {
    if s.is_empty() {
        "NIL".to_string()
    } else {
        format!("\"{}\"", escape_imap_string(s))
    }
}

/// parse an email address "Name <user@host>" or "user@host" into IMAP address structure
/// returns ((name NIL mailbox host)) or NIL if empty
pub fn format_imap_address(addr: &str) -> String {
    let addr = addr.trim();
    if addr.is_empty() {
        return "NIL".to_string();
    }

    // parse "Name <user@host>" format
    if let Some(lt) = addr.find('<') {
        let name = addr[..lt].trim().trim_matches('"');
        let email = addr[lt + 1..].trim_end_matches('>');
        let (mailbox, host) = email.split_once('@').unwrap_or((email, ""));
        let name_part = if name.is_empty() {
            "NIL".to_string()
        } else {
            format!("\"{}\"", escape_imap_string(name))
        };
        return format!(
            "(({name_part} NIL \"{}\" \"{}\"))",
            escape_imap_string(mailbox),
            escape_imap_string(host)
        );
    }

    // plain "user@host"
    if let Some((mailbox, host)) = addr.split_once('@') {
        format!(
            "((NIL NIL \"{}\" \"{}\"))",
            escape_imap_string(mailbox),
            escape_imap_string(host)
        )
    } else {
        format!("((NIL NIL \"{}\" \"\"))", escape_imap_string(addr))
    }
}

/// parse BODY[HEADER.FIELDS (field-list)] or BODY.PEEK[HEADER.FIELDS (field-list)]
/// returns (field_names, raw_section_text)
pub fn parse_header_fields_request(attributes: &str) -> Option<(Vec<String>, String)> {
    let upper = attributes.to_uppercase();
    let marker = "HEADER.FIELDS";
    let pos = upper.find(marker)?;
    let after = &attributes[pos + marker.len()..];
    let paren_start = after.find('(')?;
    let paren_end = after.find(')')?;
    let fields_str = &after[paren_start + 1..paren_end];
    let fields: Vec<String> = fields_str
        .split_whitespace()
        .map(|s| s.to_uppercase())
        .collect();
    let raw_section = format!("HEADER.FIELDS ({})", fields_str.trim());
    Some((fields, raw_section))
}

/// parse all generic `BODY\[section\]` requests like `BODY\[1\]`,
/// `BODY\[1.1\]`, `BODY\[1.MIME\]`, `BODY.PEEK\[1\]`. Returns all section
/// specifiers (e.g. `["1", "1.1", "1.MIME"]`).
pub fn parse_generic_body_sections(attributes: &str) -> Vec<String> {
    let upper = attributes.to_uppercase();
    let mut sections = Vec::new();

    for prefix in &["BODY.PEEK[", "BODY["] {
        let mut search_from = 0;
        while let Some(rel_pos) = upper[search_from..].find(prefix) {
            let abs_start = search_from + rel_pos + prefix.len();
            if let Some(end_rel) = upper[abs_start..].find(']') {
                let section = attributes[abs_start..abs_start + end_rel].trim();
                let sec_upper = section.to_uppercase();
                if !section.is_empty()
                    && sec_upper != "HEADER"
                    && sec_upper != "TEXT"
                    && !sec_upper.contains("HEADER.FIELDS")
                    && section
                        .as_bytes()
                        .first()
                        .is_some_and(|b| b.is_ascii_digit())
                {
                    let s = section.to_string();
                    if !sections.contains(&s) {
                        sections.push(s);
                    }
                }
                search_from = abs_start + end_rel + 1;
            } else {
                break;
            }
        }
    }

    sections
}

/// escape double quotes in IMAP string literals
pub fn escape_imap_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// format a comma-separated list of addresses into IMAP address list
pub fn format_addr_list(addrs: &str) -> String {
    let addrs = addrs.trim();
    if addrs.is_empty() {
        return "NIL".to_string();
    }
    let parts: Vec<String> = addrs
        .split(',')
        .map(|a| {
            let a = a.trim();
            if a.is_empty() {
                return String::new();
            }
            let formatted = format_imap_address(a);
            if formatted == "NIL" {
                return String::new();
            }
            formatted[1..formatted.len() - 1].to_string()
        })
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        "NIL".to_string()
    } else {
        format!("({})", parts.join(""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_flags_all() {
        let flags =
            FLAG_SEEN | FLAG_ANSWERED | FLAG_FLAGGED | FLAG_DELETED | FLAG_DRAFT | FLAG_RECENT;
        let s = format_imap_flags(flags);
        assert!(s.contains("\\Seen"));
        assert!(s.contains("\\Answered"));
        assert!(s.contains("\\Flagged"));
        assert!(s.contains("\\Deleted"));
        assert!(s.contains("\\Draft"));
        assert!(s.contains("\\Recent"));
    }

    #[test]
    fn format_flags_empty() {
        assert_eq!(format_imap_flags(0), "");
    }

    #[test]
    fn parse_flags_round_trip() {
        let bits = FLAG_SEEN | FLAG_FLAGGED;
        let s = format_imap_flags(bits);
        let parsed = parse_imap_flags(&format!("({})", s));
        assert_eq!(parsed, bits);
    }

    #[test]
    fn parse_flags_case_insensitive() {
        let bits = parse_imap_flags("(\\seen \\FLAGGED \\Draft)");
        assert_eq!(bits, FLAG_SEEN | FLAG_FLAGGED | FLAG_DRAFT);
    }

    #[test]
    fn parse_flags_recent_and_deleted() {
        let bits = parse_imap_flags("(\\Recent \\Deleted \\Answered)");
        assert_eq!(bits, FLAG_RECENT | FLAG_DELETED | FLAG_ANSWERED);
    }

    #[test]
    fn parse_flags_unknown_skipped() {
        let bits = parse_imap_flags("(\\Custom \\Seen)");
        assert_eq!(bits, FLAG_SEEN);
    }

    #[test]
    fn quote_or_nil_empty() {
        assert_eq!(quote_or_nil(""), "NIL");
    }

    #[test]
    fn quote_or_nil_value() {
        assert_eq!(quote_or_nil("hello"), "\"hello\"");
    }

    #[test]
    fn quote_or_nil_escapes() {
        let result = quote_or_nil("he said \"hi\"");
        assert!(result.contains("\\\""));
    }

    #[test]
    fn format_imap_address_plain() {
        let result = format_imap_address("user@example.com");
        assert!(result.contains("\"user\""));
        assert!(result.contains("\"example.com\""));
    }

    #[test]
    fn format_imap_address_with_name() {
        let result = format_imap_address("John Doe <john@example.com>");
        assert!(result.contains("\"John Doe\""));
        assert!(result.contains("\"john\""));
        assert!(result.contains("\"example.com\""));
    }

    #[test]
    fn format_imap_address_empty() {
        assert_eq!(format_imap_address(""), "NIL");
    }

    #[test]
    fn format_imap_address_with_empty_name_brackets() {
        let result = format_imap_address("<a@b.c>");
        assert!(result.contains("NIL NIL"));
        assert!(result.contains("\"a\""));
    }

    #[test]
    fn format_imap_address_no_at_sign() {
        let result = format_imap_address("plain-token");
        assert!(result.contains("\"plain-token\""));
        assert!(result.contains("\"\""));
    }

    #[test]
    fn format_addr_list_single() {
        let result = format_addr_list("user@example.com");
        assert!(result.contains("\"user\""));
        assert!(result.contains("\"example.com\""));
    }

    #[test]
    fn format_addr_list_multiple() {
        let result = format_addr_list("a@b.com, c@d.com");
        assert!(result.contains("\"a\""));
        assert!(result.contains("\"c\""));
    }

    #[test]
    fn format_addr_list_empty() {
        assert_eq!(format_addr_list(""), "NIL");
    }

    #[test]
    fn format_addr_list_with_display_name() {
        let result = format_addr_list("\"Alice\" <a@b.c>, \"Bob\" <b@c.d>");
        assert!(result.contains("\"Alice\""));
        assert!(result.contains("\"Bob\""));
    }

    #[test]
    fn parse_header_fields_request_basic() {
        let (fields, _) =
            parse_header_fields_request("BODY[HEADER.FIELDS (FROM TO SUBJECT)]").unwrap();
        assert_eq!(fields, vec!["FROM", "TO", "SUBJECT"]);
    }

    #[test]
    fn parse_generic_body_sections_basic() {
        let sections = parse_generic_body_sections("BODY[1] BODY.PEEK[2]");
        assert!(sections.contains(&"1".to_string()));
        assert!(sections.contains(&"2".to_string()));
    }

    #[test]
    fn parse_generic_body_sections_ignores_header() {
        let sections = parse_generic_body_sections("BODY[HEADER]");
        assert!(sections.is_empty());
    }

    #[test]
    fn format_internal_date_epoch() {
        let result = format_internal_date(0);
        assert!(result.contains("1970"));
    }

    #[test]
    fn escape_imap_str_basic() {
        let s = escape_imap_str("hello \"world\" \\back");
        assert!(s.contains("\\\""));
        assert!(s.contains("\\\\"));
    }
}
