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
