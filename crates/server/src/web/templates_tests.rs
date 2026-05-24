//! Tests for `templates` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn default_category_is_general() {
    assert_eq!(default_category(), "general");
}

#[test]
fn save_request_deserialize_minimal() {
    let json = r#"{"name":"test"}"#;
    let req: SaveTemplateRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.name, "test");
    assert_eq!(req.subject, "");
    assert_eq!(req.category, "general");
    assert!(!req.is_default);
}

#[test]
fn save_request_deserialize_full() {
    let json = r#"{"name":"meeting","subject":"Meeting Invite","html_body":"<p>Hi</p>","text_body":"Hi","category":"work","is_default":true}"#;
    let req: SaveTemplateRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.name, "meeting");
    assert_eq!(req.subject, "Meeting Invite");
    assert_eq!(req.category, "work");
    assert!(req.is_default);
}
