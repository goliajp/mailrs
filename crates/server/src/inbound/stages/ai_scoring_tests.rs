//! Tests for `ai_scoring` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn extract_header_basic() {
    let msg = b"From: alice@example.com\r\nSubject: Hello\r\n\r\nbody";
    assert_eq!(extract_header(msg, "Subject").unwrap(), "Hello");
    assert_eq!(extract_header(msg, "From").unwrap(), "alice@example.com");
}

#[test]
fn extract_header_case_insensitive() {
    let msg = b"subject: hello world\r\n\r\n";
    assert_eq!(extract_header(msg, "Subject").unwrap(), "hello world");
}

#[test]
fn extract_header_missing() {
    let msg = b"From: alice@example.com\r\n\r\nbody";
    assert!(extract_header(msg, "Subject").is_none());
}

#[test]
fn extract_header_empty_message() {
    assert!(extract_header(b"", "Subject").is_none());
}

#[test]
fn extract_body_preview_crlf() {
    let msg = b"Subject: Test\r\n\r\nHello, world!";
    assert_eq!(extract_body_preview(msg, 500), "Hello, world!");
}

#[test]
fn extract_body_preview_lf() {
    let msg = b"Subject: Test\n\nHello, world!";
    assert_eq!(extract_body_preview(msg, 500), "Hello, world!");
}

#[test]
fn extract_body_preview_truncates() {
    let msg = b"Subject: Test\r\n\r\nHello, world!";
    assert_eq!(extract_body_preview(msg, 5), "Hello");
}

#[test]
fn extract_body_preview_no_body() {
    let msg = b"Subject: Test\r\n";
    assert_eq!(extract_body_preview(msg, 500), "");
}

#[test]
fn extract_body_preview_empty() {
    assert_eq!(extract_body_preview(b"", 500), "");
}
