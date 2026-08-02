//! `MessageId` round-trips.

use crate::MessageId;

// --- MessageId ---

#[test]
fn message_id_display() {
    let id = MessageId("1234567890.M123P456Q0.host".to_string());
    assert_eq!(id.to_string(), "1234567890.M123P456Q0.host");
}

#[test]
fn message_id_equality() {
    let a = MessageId("abc".to_string());
    let b = MessageId("abc".to_string());
    let c = MessageId("xyz".to_string());
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn message_id_hash_consistency() {
    use std::collections::HashMap;
    let mut map: HashMap<MessageId, u32> = HashMap::new();
    map.insert(MessageId("key".to_string()), 42);
    assert_eq!(map.get(&MessageId("key".to_string())), Some(&42));
    assert_eq!(map.get(&MessageId("other".to_string())), None);
}
