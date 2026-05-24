//! Unit tests for the pure-function IMAP wire-format
//! helpers re-exported from `mailrs-imap-format`. These
//! don\'t touch the session state machine or any I/O;
//! they exercise the formatter / parser surface itself.

use mailrs_imap_format::{
    build_bodystructure, escape_imap_str, escape_imap_string, extract_body_section,
    extract_header_fields, extract_header_section, find_line_offset, format_addr_list,
    format_imap_address, format_imap_flags, format_internal_date, parse_generic_body_sections,
    parse_header_fields_request, parse_imap_flags, quote_or_nil, split_mime_parts,
    trim_part_trailing_newline,
};
use mailrs_mailbox::{
    FLAG_ANSWERED, FLAG_DELETED, FLAG_DRAFT, FLAG_FLAGGED, FLAG_RECENT, FLAG_SEEN,
};

use crate::imap_session::{imap_greeting, strs_to_bytes};

// -- format_imap_flags --

#[test]
fn format_flags_empty() {
    assert_eq!(format_imap_flags(0), "");
}

#[test]
fn format_flags_single() {
    assert_eq!(format_imap_flags(FLAG_SEEN), "\\Seen");
    assert_eq!(format_imap_flags(FLAG_DRAFT), "\\Draft");
}

#[test]
fn format_flags_multiple() {
    let s = format_imap_flags(FLAG_SEEN | FLAG_FLAGGED);
    assert_eq!(s, "\\Seen \\Flagged");
}

// -- parse_imap_flags --

#[test]
fn parse_flags_empty() {
    assert_eq!(parse_imap_flags(""), 0);
    assert_eq!(parse_imap_flags("()"), 0);
}

#[test]
fn parse_flags_without_parens() {
    assert_eq!(parse_imap_flags("\\Seen"), FLAG_SEEN);
}

#[test]
fn parse_flags_all() {
    let bits = parse_imap_flags("(\\Seen \\Answered \\Flagged \\Deleted \\Draft \\Recent)");
    assert_eq!(
        bits,
        FLAG_SEEN | FLAG_ANSWERED | FLAG_FLAGGED | FLAG_DELETED | FLAG_DRAFT | FLAG_RECENT
    );
}

#[test]
fn parse_flags_case_insensitive() {
    assert_eq!(parse_imap_flags("(\\seen \\FLAGGED)"), FLAG_SEEN | FLAG_FLAGGED);
}

#[test]
fn parse_flags_unknown_ignored() {
    assert_eq!(parse_imap_flags("(\\Seen \\CustomFlag)"), FLAG_SEEN);
}

// -- format_imap_flags / parse_imap_flags roundtrip --

#[test]
fn flags_roundtrip() {
    let original = FLAG_SEEN | FLAG_ANSWERED | FLAG_FLAGGED | FLAG_DELETED | FLAG_DRAFT | FLAG_RECENT;
    let formatted = format_imap_flags(original);
    let parsed = parse_imap_flags(&format!("({})", formatted));
    assert_eq!(parsed, original);
}

// -- escape_imap_string --

#[test]
fn escape_plain_string() {
    assert_eq!(escape_imap_string("hello"), "hello");
}

#[test]
fn escape_quotes_and_backslashes() {
    assert_eq!(escape_imap_string(r#"say "hi""#), r#"say \"hi\""#);
    assert_eq!(escape_imap_string(r"path\to"), r"path\\to");
}

// -- quote_or_nil --

#[test]
fn quote_or_nil_empty() {
    assert_eq!(quote_or_nil(""), "NIL");
}

#[test]
fn quote_or_nil_non_empty() {
    assert_eq!(quote_or_nil("hello"), "\"hello\"");
}

#[test]
fn quote_or_nil_special_chars() {
    assert_eq!(quote_or_nil(r#"a"b"#), r#""a\"b""#);
}

// -- format_imap_address --

#[test]
fn address_no_at() {
    assert_eq!(format_imap_address("localonly"), "((NIL NIL \"localonly\" \"\"))");
}

#[test]
fn address_with_quoted_name() {
    let result = format_imap_address("\"Bob Smith\" <bob@example.com>");
    assert_eq!(result, "((\"Bob Smith\" NIL \"bob\" \"example.com\"))");
}

#[test]
fn address_name_without_quotes() {
    let result = format_imap_address("Bob Smith <bob@example.com>");
    assert_eq!(result, "((\"Bob Smith\" NIL \"bob\" \"example.com\"))");
}

#[test]
fn address_angle_bracket_no_name() {
    let result = format_imap_address("<alice@example.com>");
    assert_eq!(result, "((NIL NIL \"alice\" \"example.com\"))");
}

// -- format_addr_list --

#[test]
fn addr_list_empty() {
    assert_eq!(format_addr_list(""), "NIL");
    assert_eq!(format_addr_list("  "), "NIL");
}

#[test]
fn addr_list_single() {
    let result = format_addr_list("alice@example.com");
    assert_eq!(result, "((NIL NIL \"alice\" \"example.com\"))");
}

#[test]
fn addr_list_with_names() {
    let result = format_addr_list("Alice <alice@a.com>, Bob <bob@b.com>");
    assert!(result.starts_with('('));
    assert!(result.ends_with(')'));
    assert!(result.contains("\"Alice\""));
    assert!(result.contains("\"Bob\""));
}

// -- imap_greeting --

#[test]
fn greeting_format() {
    let g = imap_greeting("mail.example.com");
    let s = String::from_utf8(g).unwrap();
    assert!(s.starts_with("* OK"));
    assert!(s.contains("mail.example.com"));
    assert!(s.contains("IMAP4rev1"));
    assert!(s.ends_with("\r\n"));
}

// -- strs_to_bytes --

#[test]
fn strs_to_bytes_empty() {
    let result = strs_to_bytes(vec![]);
    assert!(result.is_empty());
}

#[test]
fn strs_to_bytes_converts() {
    let result = strs_to_bytes(vec!["hello".into(), "world".into()]);
    assert_eq!(result, vec![b"hello".to_vec(), b"world".to_vec()]);
}

// -- format_internal_date --

#[test]
fn format_internal_date_known_timestamp() {
    let result = format_internal_date(0);
    // unix epoch: 1970-01-01
    assert!(result.contains("1970"));
    assert!(result.contains("Jan"));
}

#[test]
fn format_internal_date_recent() {
    let result = format_internal_date(1700000000);
    // 2023-11-14 in UTC
    assert!(result.contains("2023"));
    assert!(result.contains("Nov"));
}

// -- extract_header_section --

#[test]
fn extract_header_crlf() {
    let data = b"From: alice\r\nTo: bob\r\n\r\nBody here";
    let header = extract_header_section(data);
    assert_eq!(header, b"From: alice\r\nTo: bob\r\n\r\n");
}

#[test]
fn extract_header_lf_only() {
    let data = b"From: alice\nTo: bob\n\nBody here";
    let header = extract_header_section(data);
    assert_eq!(header, b"From: alice\nTo: bob\n\n");
}

#[test]
fn extract_header_no_separator() {
    let data = b"From: alice\r\nTo: bob";
    let header = extract_header_section(data);
    assert_eq!(header, data.to_vec());
}

// -- extract_body_section --

#[test]
fn extract_body_crlf() {
    let data = b"From: alice\r\n\r\nBody content";
    let body = extract_body_section(data);
    assert_eq!(body, b"Body content");
}

#[test]
fn extract_body_lf_only() {
    let data = b"From: alice\n\nBody content";
    let body = extract_body_section(data);
    assert_eq!(body, b"Body content");
}

#[test]
fn extract_body_no_separator() {
    let data = b"From: alice";
    let body = extract_body_section(data);
    assert!(body.is_empty());
}

// -- extract_header_fields --

#[test]
fn extract_specific_headers() {
    let data = b"From: alice@example.com\r\nTo: bob@example.com\r\nSubject: Test\r\nDate: Mon, 1 Jan 2024\r\n\r\nBody";
    let fields = vec!["FROM".into(), "SUBJECT".into()];
    let result = extract_header_fields(data, &fields);
    let s = String::from_utf8(result).unwrap();
    assert!(s.contains("From: alice@example.com"));
    assert!(s.contains("Subject: Test"));
    assert!(!s.contains("To:"));
    assert!(!s.contains("Date:"));
}

#[test]
fn extract_header_fields_with_continuation() {
    let data = b"Subject: This is a\r\n very long subject\r\nFrom: alice\r\n\r\nBody";
    let fields = vec!["SUBJECT".into()];
    let result = extract_header_fields(data, &fields);
    let s = String::from_utf8(result).unwrap();
    assert!(s.contains("Subject: This is a"));
    assert!(s.contains("very long subject"));
    assert!(!s.contains("From:"));
}

// -- parse_header_fields_request --

#[test]
fn parse_header_fields_basic() {
    let input = "BODY[HEADER.FIELDS (FROM TO SUBJECT)]";
    let (fields, raw) = parse_header_fields_request(input).unwrap();
    assert_eq!(fields, vec!["FROM", "TO", "SUBJECT"]);
    assert_eq!(raw, "HEADER.FIELDS (FROM TO SUBJECT)");
}

#[test]
fn parse_header_fields_peek() {
    let input = "BODY.PEEK[HEADER.FIELDS (DATE FROM)]";
    let (fields, _raw) = parse_header_fields_request(input).unwrap();
    assert_eq!(fields, vec!["DATE", "FROM"]);
}

#[test]
fn parse_header_fields_no_match() {
    assert!(parse_header_fields_request("BODY[]").is_none());
    assert!(parse_header_fields_request("FLAGS").is_none());
}

// -- parse_generic_body_sections --

#[test]
fn parse_body_section_numeric() {
    let sections = parse_generic_body_sections("BODY[1]");
    assert_eq!(sections, vec!["1"]);
}

#[test]
fn parse_body_section_nested() {
    let sections = parse_generic_body_sections("BODY[1.1] BODY[2]");
    assert_eq!(sections, vec!["1.1", "2"]);
}

#[test]
fn parse_body_section_peek() {
    let sections = parse_generic_body_sections("BODY.PEEK[1.MIME]");
    assert_eq!(sections, vec!["1.MIME"]);
}

#[test]
fn parse_body_section_skips_header_text() {
    let sections = parse_generic_body_sections("BODY[HEADER] BODY[TEXT] BODY[HEADER.FIELDS (FROM)]");
    assert!(sections.is_empty());
}

#[test]
fn parse_body_section_empty() {
    let sections = parse_generic_body_sections("BODY[]");
    assert!(sections.is_empty());
}

#[test]
fn parse_body_section_deduplicates() {
    let sections = parse_generic_body_sections("BODY[1] BODY.PEEK[1]");
    assert_eq!(sections, vec!["1"]);
}

// -- find_line_offset --

#[test]
fn find_line_offset_first_line() {
    let data = b"line0\nline1\nline2\n";
    assert_eq!(find_line_offset(data,0), Some(0));
}

#[test]
fn find_line_offset_middle() {
    let data = b"line0\nline1\nline2\n";
    assert_eq!(find_line_offset(data,1), Some(6));
    assert_eq!(find_line_offset(data,2), Some(12));
}

#[test]
fn find_line_offset_past_end() {
    let data = b"line0\nline1\n";
    assert_eq!(find_line_offset(data,10), None);
}

// -- trim_part_trailing_newline --

#[test]
fn trim_trailing_crlf() {
    assert_eq!(trim_part_trailing_newline(b"data\r\n"), b"data");
}

#[test]
fn trim_trailing_lf() {
    assert_eq!(trim_part_trailing_newline(b"data\n"), b"data");
}

#[test]
fn trim_trailing_no_newline() {
    assert_eq!(trim_part_trailing_newline(b"data"), b"data");
}

#[test]
fn trim_trailing_empty() {
    assert_eq!(trim_part_trailing_newline(b""), b"");
}

// -- escape_imap_str (the second one) --

#[test]
fn escape_imap_str_basic() {
    assert_eq!(escape_imap_str("plain"), "plain");
    assert_eq!(escape_imap_str(r#"a"b\c"#), r#"a\"b\\c"#);
}

// -- split_mime_parts --

#[test]
fn split_mime_simple() {
    let body = b"--boundary\r\nContent-Type: text/plain\r\n\r\npart1\r\n--boundary\r\nContent-Type: text/html\r\n\r\npart2\r\n--boundary--\r\n";
    let parts = split_mime_parts(body, "boundary");
    assert_eq!(parts.len(), 2);
    assert!(String::from_utf8_lossy(parts[0]).contains("part1"));
    assert!(String::from_utf8_lossy(parts[1]).contains("part2"));
}

#[test]
fn split_mime_no_parts() {
    let body = b"no boundaries here";
    let parts = split_mime_parts(body, "boundary");
    assert!(parts.is_empty());
}

// -- build_bodystructure (basic smoke test) --

#[test]
fn build_bodystructure_text_plain() {
    let msg = b"Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 7bit\r\n\r\nHello world";
    let bs = build_bodystructure(msg);
    let upper = bs.to_uppercase();
    assert!(upper.contains("TEXT"));
    assert!(upper.contains("PLAIN"));
}

#[test]
fn build_bodystructure_multipart() {
    let msg = b"Content-Type: multipart/alternative; boundary=\"abc\"\r\n\r\n--abc\r\nContent-Type: text/plain\r\n\r\nplain\r\n--abc\r\nContent-Type: text/html\r\n\r\n<b>html</b>\r\n--abc--\r\n";
    let bs = build_bodystructure(msg);
    let upper = bs.to_uppercase();
    assert!(upper.contains("ALTERNATIVE"));
    assert!(upper.contains("PLAIN"));
    assert!(upper.contains("HTML"));
}

