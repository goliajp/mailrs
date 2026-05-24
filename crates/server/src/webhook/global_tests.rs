//! Tests for `global` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn payload_serializes_to_expected_format() {
    let payload = build_payload(
        "lihao@golia.jp",
        "sender@example.com",
        "Hello",
        "First 100 chars",
    );
    let json = serde_json::to_value(&payload).unwrap();

    assert_eq!(json["event"], "new_mail");
    assert_eq!(json["address"], "lihao@golia.jp");
    assert_eq!(json["count"], 1);
    assert_eq!(json["latest"]["from"], "sender@example.com");
    assert_eq!(json["latest"]["subject"], "Hello");
    assert_eq!(json["latest"]["snippet"], "First 100 chars");
    assert!(json["latest"]["timestamp"].is_number());
}

#[test]
fn payload_event_is_new_mail() {
    let payload = build_payload("a@b.com", "c@d.com", "Sub", "Snip");
    assert_eq!(payload.event, "new_mail");
    assert_eq!(payload.count, 1);
}
