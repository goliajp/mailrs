//! When each agent key was last used, to the hour.
//!
//! The answer to "does a service still run on this key", shown before a
//! compromise reset revokes them all. It is kept in its own hash rather
//! than on the key's record: a write to the record racing a revocation
//! would put the revoked record back, and the key with it. A key is
//! written at most once an hour; `None` means nothing was recorded, which
//! is every key before the release that started recording.

use std::collections::HashMap;
use std::sync::Mutex;

use kevy_client::Connection;

/// How stale a recorded time may get before the next use rewrites it.
pub(crate) const RESOLUTION_SECS: i64 = 3600;

pub(crate) fn hash_key(owner: &str) -> String {
    format!("agent:keys:last_used:{owner}")
}

/// When this process last wrote each key, so a busy key costs a write an
/// hour rather than a read on every request.
static WRITTEN: Mutex<Option<HashMap<(String, i64), i64>>> = Mutex::new(None);

/// Note a use of `owner`'s key `id` at `now`.
pub(crate) fn note_use(c: &mut Connection, owner: &str, id: i64, now: i64) -> std::io::Result<()> {
    if !due(owner, id, now) {
        return Ok(());
    }
    c.hset(
        hash_key(owner).as_bytes(),
        &[(id.to_string().as_bytes(), now.to_string().as_bytes())],
    )?;
    Ok(())
}

fn due(owner: &str, id: i64, now: i64) -> bool {
    let mut guard = WRITTEN.lock().unwrap_or_else(|p| p.into_inner());
    let written = guard.get_or_insert_with(HashMap::new);
    let key = (owner.to_string(), id);
    if written
        .get(&key)
        .is_some_and(|at| now - at < RESOLUTION_SECS)
    {
        return false;
    }
    written.insert(key, now);
    true
}

/// Every recorded time of `owner`'s keys, by id.
pub(crate) fn of_owner(c: &mut Connection, owner: &str) -> std::io::Result<HashMap<i64, i64>> {
    let mut out = HashMap::new();
    let raw = c.hgetall(hash_key(owner).as_bytes())?;
    for pair in raw.chunks(2) {
        let [id, at] = pair else { continue };
        let id = std::str::from_utf8(id).ok().and_then(|s| s.parse().ok());
        let at = std::str::from_utf8(at).ok().and_then(|s| s.parse().ok());
        if let (Some(id), Some(at)) = (id, at) {
            out.insert(id, at);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_written_at_most_once_an_hour() {
        assert!(due("usage@example.test", 1, 10_000));
        assert!(!due("usage@example.test", 1, 10_000 + RESOLUTION_SECS - 1));
        assert!(due("usage@example.test", 1, 10_000 + RESOLUTION_SECS));
        // another key of the same owner is its own
        assert!(due("usage@example.test", 2, 10_001));
    }
}
