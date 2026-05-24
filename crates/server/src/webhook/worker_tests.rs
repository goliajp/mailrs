//! Tests for `worker` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn build_headers_includes_all_required_headers() {
    let payload = br#"{"event":"new_message"}"#;
    let headers = build_headers("my-secret", payload, "new_message", 42);

    assert_eq!(headers.len(), 4);

    let names: Vec<&str> = headers.iter().map(|(k, _)| k.as_str()).collect();
    assert!(names.contains(&"Content-Type"));
    assert!(names.contains(&"X-Mailrs-Signature"));
    assert!(names.contains(&"X-Mailrs-Event"));
    assert!(names.contains(&"X-Mailrs-Delivery"));
}

#[test]
fn build_headers_content_type_is_json() {
    let headers = build_headers("s", b"p", "new_message", 1);
    let ct = headers.iter().find(|(k, _)| k == "Content-Type").unwrap();
    assert_eq!(ct.1, "application/json");
}

#[test]
fn build_headers_signature_has_sha256_prefix() {
    let headers = build_headers("secret", b"payload", "new_message", 1);
    let sig = headers.iter().find(|(k, _)| k == "X-Mailrs-Signature").unwrap();
    assert!(sig.1.starts_with("sha256="));
}

#[test]
fn build_headers_event_matches_input() {
    let headers = build_headers("s", b"p", "new_message", 1);
    let evt = headers.iter().find(|(k, _)| k == "X-Mailrs-Event").unwrap();
    assert_eq!(evt.1, "new_message");
}

#[test]
fn build_headers_delivery_id_matches() {
    let headers = build_headers("s", b"p", "new_message", 999);
    let del = headers.iter().find(|(k, _)| k == "X-Mailrs-Delivery").unwrap();
    assert_eq!(del.1, "999");
}

#[test]
fn build_headers_signature_is_deterministic() {
    let h1 = build_headers("secret", b"payload", "new_message", 1);
    let h2 = build_headers("secret", b"payload", "new_message", 1);
    let sig1 = &h1.iter().find(|(k, _)| k == "X-Mailrs-Signature").unwrap().1;
    let sig2 = &h2.iter().find(|(k, _)| k == "X-Mailrs-Signature").unwrap().1;
    assert_eq!(sig1, sig2);
}

#[test]
fn build_headers_different_secrets_produce_different_signatures() {
    let h1 = build_headers("secret1", b"payload", "new_message", 1);
    let h2 = build_headers("secret2", b"payload", "new_message", 1);
    let sig1 = &h1.iter().find(|(k, _)| k == "X-Mailrs-Signature").unwrap().1;
    let sig2 = &h2.iter().find(|(k, _)| k == "X-Mailrs-Signature").unwrap().1;
    assert_ne!(sig1, sig2);
}
