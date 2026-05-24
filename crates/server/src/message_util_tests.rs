//! Tests for `message_util` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn extract_header_simple() {
    let raw = b"From: alice@example.com\r\nTo: bob@example.com\r\n\r\nbody";
    assert_eq!(extract_header_from_raw(raw, "From"), "alice@example.com");
    assert_eq!(extract_header_from_raw(raw, "To"), "bob@example.com");
}

#[test]
fn extract_header_case_insensitive() {
    let raw = b"FROM: alice@example.com\r\n\r\n";
    assert_eq!(extract_header_from_raw(raw, "from"), "alice@example.com");
}

#[test]
fn extract_header_folded() {
    let raw = b"Subject: This is a very long\r\n subject line\r\n\r\nbody";
    assert_eq!(extract_header_from_raw(raw, "Subject"), "This is a very long subject line");
}

#[test]
fn extract_header_folded_tab() {
    let raw = b"To: alice@example.com,\r\n\tbob@example.com\r\n\r\n";
    assert_eq!(extract_header_from_raw(raw, "To"), "alice@example.com, bob@example.com");
}

#[test]
fn extract_header_missing() {
    let raw = b"From: alice@example.com\r\n\r\n";
    assert_eq!(extract_header_from_raw(raw, "Subject"), "");
}

#[test]
fn decode_header_plain() {
    assert_eq!(decode_header("Hello World"), "Hello World");
}

#[test]
fn decode_header_rfc2047_utf8() {
    let encoded = "=?UTF-8?B?5pel5pys6Kqe?=";
    assert_eq!(decode_header(encoded), "日本語");
}

#[test]
fn parse_message_plain_text() {
    let raw = b"From: a@b.com\r\nSubject: test\r\nContent-Type: text/plain\r\n\r\nHello";
    let (text, html, atts) = parse_message(raw);
    // mail_parser may or may not extract text from minimal messages
    // just check it doesn't panic and returns some result
    let _ = (text, html);
    assert!(atts.is_empty());
}

#[test]
fn parse_message_with_body() {
    let raw = b"From: a@b.com\r\nTo: c@d.com\r\nSubject: test\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nHello World";
    let (text, _html, _atts) = parse_message(raw);
    assert!(text.is_some());
    assert!(text.unwrap().contains("Hello"));
}
