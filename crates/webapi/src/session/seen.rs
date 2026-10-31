//! When a session was last used, for its owner's list of sessions.

/// How stale a session's `last_seen_unix` may get before a request
/// rewrites it.
const SEEN_EVERY_SECS: i64 = 600;

/// The session record with `last_seen_unix` set to now, when it is more
/// than [`SEEN_EVERY_SECS`] behind; `None` when it is recent enough.
pub(super) fn seen_now(blob: &[u8]) -> Option<Vec<u8>> {
    let mut v: serde_json::Value = serde_json::from_slice(blob).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    let seen = v
        .get("last_seen_unix")
        .and_then(|s| s.as_i64())
        .unwrap_or(0);
    if now - seen < SEEN_EVERY_SECS {
        return None;
    }
    v.as_object_mut()?
        .insert("last_seen_unix".into(), now.into());
    serde_json::to_vec(&v).ok()
}
