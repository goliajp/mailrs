//! The limits the session document declares, and their enforcement.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use mailrs_jmap::error::JmapMethodError;

/// `maxSizeRequest`.
pub const MAX_SIZE_REQUEST: usize = 10_000_000;
/// `maxConcurrentRequests`, per account.
pub const MAX_CONCURRENT_REQUESTS: usize = 4;
/// `maxCallsInRequest`.
pub const MAX_CALLS: usize = 16;
/// `maxObjectsInGet` and `maxObjectsInSet`.
pub const MAX_OBJECTS: usize = 500;
/// `maxSizeUpload`.
pub const MAX_SIZE_UPLOAD: usize = 50 * 1024 * 1024;
/// `maxConcurrentUpload`, per account.
pub const MAX_CONCURRENT_UPLOAD: usize = 4;

/// The request-level error type for a limit (RFC 8620 §3.6.1).
pub const LIMIT: &str = "urn:ietf:params:jmap:error:limit";

/// A method-level error for more objects than one call may name.
pub fn limit_error(name: &str) -> JmapMethodError {
    JmapMethodError::Other {
        kind: "requestTooLarge".into(),
        description: format!("more than {name} ({MAX_OBJECTS})"),
    }
}

fn counts() -> &'static Mutex<HashMap<(&'static str, String), usize>> {
    static IN_FLIGHT: OnceLock<Mutex<HashMap<(&'static str, String), usize>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(|| Mutex::new(HashMap::new()))
}

/// One of an account's concurrent requests (or uploads), given back when
/// dropped.
pub struct Slot {
    kind: &'static str,
    user: String,
}

impl Slot {
    /// One of an account's open push connections.
    pub fn take_push(user: &str, max: usize) -> Option<Slot> {
        Slot::take("push", user, max)
    }

    fn take(kind: &'static str, user: &str, max: usize) -> Option<Slot> {
        let mut map = counts().lock().unwrap_or_else(|e| e.into_inner());
        let n = map.entry((kind, user.to_string())).or_insert(0);
        if *n >= max {
            return None;
        }
        *n += 1;
        Some(Slot {
            kind,
            user: user.to_string(),
        })
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut map = counts().lock().unwrap_or_else(|e| e.into_inner());
        let key = (self.kind, std::mem::take(&mut self.user));
        if let Some(n) = map.get_mut(&key) {
            *n -= 1;
            if *n == 0 {
                map.remove(&key);
            }
        }
    }
}

pub struct RequestSlot;

impl RequestSlot {
    pub fn take(user: &str) -> Option<Slot> {
        Slot::take("request", user, MAX_CONCURRENT_REQUESTS)
    }
}

pub struct UploadSlot;

impl UploadSlot {
    pub fn take(user: &str) -> Option<Slot> {
        Slot::take("upload", user, MAX_CONCURRENT_UPLOAD)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_account_has_at_most_its_share_of_requests_at_once() {
        let held: Vec<Slot> = (0..MAX_CONCURRENT_REQUESTS)
            .map(|_| RequestSlot::take("limits-a@x.test").expect("free"))
            .collect();
        assert!(RequestSlot::take("limits-a@x.test").is_none());
        assert!(
            RequestSlot::take("limits-b@x.test").is_some(),
            "another account is not held up"
        );
        assert!(
            UploadSlot::take("limits-a@x.test").is_some(),
            "uploads are counted apart"
        );
        drop(held);
        assert!(RequestSlot::take("limits-a@x.test").is_some());
    }
}
