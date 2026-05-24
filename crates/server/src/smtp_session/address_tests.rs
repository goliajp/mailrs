//! Tests for `address` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn empty_list_treats_all_as_local() {
    assert!(is_local_domain("anything.com", &[]));
}

#[test]
fn exact_match() {
    let domains = vec!["example.com".into()];
    assert!(is_local_domain("example.com", &domains));
    assert!(!is_local_domain("other.com", &domains));
}

#[test]
fn case_insensitive() {
    let domains = vec!["example.com".into()];
    assert!(is_local_domain("Example.COM", &domains));
    assert!(is_local_domain("EXAMPLE.COM", &domains));
}

#[test]
fn multiple_domains() {
    let domains = vec!["a.com".into(), "b.com".into()];
    assert!(is_local_domain("a.com", &domains));
    assert!(is_local_domain("b.com", &domains));
    assert!(!is_local_domain("c.com", &domains));
}

#[test]
fn subdomain_not_matched() {
    let domains = vec!["example.com".into()];
    assert!(!is_local_domain("sub.example.com", &domains));
}
