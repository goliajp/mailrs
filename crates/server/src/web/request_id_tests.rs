//! Tests for `request_id` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn generate_request_id_length() {
    let id = generate_request_id();
    assert_eq!(id.len(), 32);
}

#[test]
fn generate_request_id_is_hex() {
    let id = generate_request_id();
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn generate_request_id_unique() {
    let a = generate_request_id();
    let b = generate_request_id();
    assert_ne!(a, b);
}

#[test]
fn hex_encode_empty() {
    assert_eq!(hex_encode(&[]), "");
}

#[test]
fn hex_encode_single_byte() {
    assert_eq!(hex_encode(&[0xff]), "ff");
    assert_eq!(hex_encode(&[0x00]), "00");
    assert_eq!(hex_encode(&[0x0a]), "0a");
}

#[test]
fn hex_encode_multiple_bytes() {
    assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
}

#[test]
fn is_valid_request_id_alphanumeric() {
    assert!(is_valid_request_id("abc123"));
}

#[test]
fn is_valid_request_id_with_special_chars() {
    assert!(is_valid_request_id("abc-123_456.789"));
}

#[test]
fn is_valid_request_id_empty() {
    assert!(!is_valid_request_id(""));
}

#[test]
fn is_valid_request_id_too_long() {
    let long = "a".repeat(129);
    assert!(!is_valid_request_id(&long));
}

#[test]
fn is_valid_request_id_max_length() {
    let max = "a".repeat(128);
    assert!(is_valid_request_id(&max));
}

#[test]
fn is_valid_request_id_rejects_spaces() {
    assert!(!is_valid_request_id("abc 123"));
}

#[test]
fn is_valid_request_id_rejects_newlines() {
    assert!(!is_valid_request_id("abc\n123"));
}

#[test]
fn is_valid_request_id_rejects_unicode() {
    assert!(!is_valid_request_id("日本語"));
}

#[test]
fn is_valid_request_id_rejects_slashes() {
    assert!(!is_valid_request_id("abc/123"));
}

#[test]
fn is_valid_request_id_rejects_colons() {
    assert!(!is_valid_request_id("abc:123"));
}
