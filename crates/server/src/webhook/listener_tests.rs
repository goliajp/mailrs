//! Tests for `listener` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use chrono::Utc;
use super::*;

fn make_sub(
    filter_sender: Option<&str>,
    filter_thread_id: Option<&str>,
) -> Subscription {
    Subscription {
        id: 1,
        account_address: "user@example.com".to_string(),
        url: "https://example.com/hook".to_string(),
        event_type: "new_message".to_string(),
        filter_sender: filter_sender.map(|s| s.to_string()),
        filter_thread_id: filter_thread_id.map(|s| s.to_string()),
        signing_secret: "secret".to_string(),
        active: true,
        created_at: Utc::now(),
    }
}

#[test]
fn no_filter_matches_any_sender_and_thread() {
    let sub = make_sub(None, None);
    assert!(matches_subscription(&sub, "anyone@example.com", "any-thread"));
    assert!(matches_subscription(&sub, "other@example.com", "other-thread"));
}

#[test]
fn filter_sender_matches_only_exact_sender() {
    let sub = make_sub(Some("specific@example.com"), None);
    assert!(matches_subscription(&sub, "specific@example.com", "any-thread"));
    assert!(!matches_subscription(&sub, "other@example.com", "any-thread"));
}

#[test]
fn filter_thread_id_matches_only_exact_thread() {
    let sub = make_sub(None, Some("thread-123"));
    assert!(matches_subscription(&sub, "anyone@example.com", "thread-123"));
    assert!(!matches_subscription(&sub, "anyone@example.com", "thread-456"));
}

#[test]
fn both_filters_require_both_to_match() {
    let sub = make_sub(Some("specific@example.com"), Some("thread-123"));
    assert!(matches_subscription(&sub, "specific@example.com", "thread-123"));
    assert!(!matches_subscription(&sub, "specific@example.com", "thread-456"));
    assert!(!matches_subscription(&sub, "other@example.com", "thread-123"));
    assert!(!matches_subscription(&sub, "other@example.com", "thread-456"));
}

#[test]
fn build_payload_creates_correct_structure() {
    let payload = build_payload("user@ex.com", "t1", "sender@ex.com", "Hello", "Snippet...");
    assert_eq!(payload.event, "new_message");
    assert_eq!(payload.data.user, "user@ex.com");
    assert_eq!(payload.data.thread_id, "t1");
    assert_eq!(payload.data.sender, "sender@ex.com");
    assert_eq!(payload.data.subject, "Hello");
    assert_eq!(payload.data.snippet, "Snippet...");
    // timestamp should be parseable
    assert!(chrono::DateTime::parse_from_rfc3339(&payload.timestamp).is_ok());
}
