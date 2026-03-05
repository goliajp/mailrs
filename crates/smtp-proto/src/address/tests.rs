use crate::address::{is_valid, split_address};

// --- validation ---

#[test]
fn valid_simple() {
    assert!(is_valid("user@example.com"));
}

#[test]
fn valid_subdomain() {
    assert!(is_valid("user@mail.sub.example.com"));
}

#[test]
fn valid_plus_addressing() {
    assert!(is_valid("user+tag@example.com"));
}

#[test]
fn invalid_no_at() {
    assert!(!is_valid("userexample.com"));
}

#[test]
fn invalid_no_local() {
    assert!(!is_valid("@example.com"));
}

#[test]
fn invalid_no_domain() {
    assert!(!is_valid("user@"));
}

// --- split ---

#[test]
fn split_address_normal() {
    let (local, domain) = split_address("user@example.com").unwrap();
    assert_eq!(local, "user");
    assert_eq!(domain, "example.com");
}

#[test]
fn split_address_invalid() {
    assert!(split_address("noatsign").is_none());
}

// --- domain matching ---

#[test]
fn is_local_domain_match() {
    let locals = ["example.com", "mail.golia.jp"];
    let (_, domain) = split_address("user@example.com").unwrap();
    assert!(locals.contains(&domain));
}

#[test]
fn is_local_domain_no_match() {
    let locals = ["example.com", "mail.golia.jp"];
    let (_, domain) = split_address("user@other.org").unwrap();
    assert!(!locals.contains(&domain));
}

// --- split_address edge cases ---

#[test]
fn split_address_empty_local() {
    // "@domain.com" — empty local part should return None
    assert!(split_address("@domain.com").is_none());
}

#[test]
fn split_address_empty_domain() {
    // "user@" — empty domain part should return None
    assert!(split_address("user@").is_none());
}

#[test]
fn split_address_multiple_at_signs() {
    // only the first '@' is used as split point
    let result = split_address("user@host@extra.com");
    // local = "user", domain = "host@extra.com" — both non-empty, so Some
    assert!(result.is_some());
    let (local, domain) = result.unwrap();
    assert_eq!(local, "user");
    assert_eq!(domain, "host@extra.com");
}

#[test]
fn is_valid_empty_string() {
    assert!(!is_valid(""));
}

#[test]
fn is_valid_only_at() {
    assert!(!is_valid("@"));
}

#[test]
fn is_valid_numeric_local() {
    assert!(is_valid("123@example.com"));
}

#[test]
fn is_valid_hyphenated_domain() {
    assert!(is_valid("user@mail-server.example.com"));
}
