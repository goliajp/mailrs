//! Tests for `conversation_cache` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn list_key_normalizes_domain_order() {
    let a = list_key(
        "alice@x.com",
        50,
        None,
        None,
        Some(&["a.com".into(), "b.com".into()]),
        None,
        None,
        None,
        None,
        None,
    );
    let b = list_key(
        "alice@x.com",
        50,
        None,
        None,
        Some(&["b.com".into(), "a.com".into()]),
        None,
        None,
        None,
        None,
        None,
    );
    assert_eq!(a, b, "domain order should not affect the cache key");
}

#[test]
fn list_key_distinguishes_filter_combinations() {
    let inbox = list_key(
        "alice@x.com",
        50,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let sent = list_key(
        "alice@x.com",
        50,
        None,
        None,
        None,
        None,
        Some("Sent"),
        None,
        None,
        None,
    );
    assert_ne!(inbox, sent);
}

#[test]
fn thread_key_namespaces_by_user() {
    assert_ne!(
        thread_key("alice@x.com", "t1"),
        thread_key("bob@x.com", "t1"),
        "different users must have distinct thread keys"
    );
}

#[test]
fn categories_key_normalizes_empty_domains() {
    assert_eq!(
        categories_key("alice@x.com", None),
        categories_key("alice@x.com", Some(&[]))
    );
}
