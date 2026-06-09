// Cache-aside Kevy layer for the hot conversation read endpoints.
//
// Keys are segmented per user so a single invalidation scoped to that
// user is cheap (SCAN/DEL only touches their keys).
//
// We cache the *already-serialized JSON response bytes*. That lets us:
//   1. skip re-serializing on every cache hit (the response is a
//      ContentType: application/json header + raw bytes)
//   2. cache without adding serde derives to all the internal data types
//   3. trivially evict via DEL — no value-shape coupling
//
// Reads:
//   mailrs:list:{user}:{filter_hash}   → JSON of get_conversations
//   mailrs:thread:{user}:{thread_id}   → JSON of get_thread_messages
//   mailrs:cats:{user}:{domains_csv}   → JSON of get_conversation_categories
//   mailrs:action:{user}:{domains_csv} → JSON of get_action_count
//
// Invalidation:
//   - any mutation on a user's threads → bust_user (drops list / cats / action)
//     plus DEL on the specific thread key if the mutation references one.
//   - SMTP inbound delivery → bust_user once at end of inbound pipeline.
//   - 30s list TTL caps staleness even if a write path forgets to bust.

use crate::kevy_store::KevyStore;

const PREFIX_LIST: &str = "mailrs:list";
const PREFIX_THREAD: &str = "mailrs:thread";
const PREFIX_CATS: &str = "mailrs:cats";
const PREFIX_ACTION: &str = "mailrs:action";

pub const TTL_LIST_SECS: u64 = 30;
pub const TTL_THREAD_SECS: u64 = 300;
pub const TTL_CATS_SECS: u64 = 60;
pub const TTL_ACTION_SECS: u64 = 60;

/// Short, collision-resistant-enough hash for filter combinations in keys.
/// Not crypto; we just want different filter sets to map to different keys.
fn short_hash(s: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

#[allow(clippy::too_many_arguments)]
pub fn list_key(
    user: &str,
    limit: u32,
    before: Option<i64>,
    category: Option<&str>,
    domains: Option<&[String]>,
    archived: Option<bool>,
    folder: Option<&str>,
    unread: Option<bool>,
    starred: Option<bool>,
    section: Option<&str>,
) -> String {
    let mut filter = String::with_capacity(128);
    filter.push_str(&limit.to_string());
    if let Some(b) = before {
        filter.push_str("|b=");
        filter.push_str(&b.to_string());
    }
    if let Some(c) = category {
        filter.push_str("|c=");
        filter.push_str(c);
    }
    if let Some(d) = domains {
        let mut sorted: Vec<&str> = d.iter().map(|s| s.as_str()).collect();
        sorted.sort_unstable();
        filter.push_str("|d=");
        filter.push_str(&sorted.join(","));
    }
    if archived.unwrap_or(false) {
        filter.push_str("|a");
    }
    if let Some(f) = folder {
        filter.push_str("|f=");
        filter.push_str(f);
    }
    if unread.unwrap_or(false) {
        filter.push_str("|u");
    }
    if starred.unwrap_or(false) {
        filter.push_str("|s");
    }
    if let Some(s) = section {
        filter.push_str("|sec=");
        filter.push_str(s);
    }
    format!("{}:{}:{}", PREFIX_LIST, user, short_hash(&filter))
}

pub fn thread_key(user: &str, thread_id: &str) -> String {
    format!("{}:{}:{}", PREFIX_THREAD, user, thread_id)
}

pub fn categories_key(user: &str, domains: Option<&[String]>) -> String {
    format!("{}:{}:{}", PREFIX_CATS, user, domains_part(domains))
}

pub fn action_count_key(user: &str, domains: Option<&[String]>) -> String {
    format!("{}:{}:{}", PREFIX_ACTION, user, domains_part(domains))
}

fn domains_part(domains: Option<&[String]>) -> String {
    match domains {
        Some(d) if !d.is_empty() => {
            let mut sorted: Vec<&str> = d.iter().map(|s| s.as_str()).collect();
            sorted.sort_unstable();
            sorted.join(",")
        }
        _ => String::new(),
    }
}

/// Get a cached JSON response body. None on miss / decode error.
pub fn get_json(kevy: &KevyStore, key: &str) -> Option<String> {
    let bytes = kevy.get(key.as_bytes()).ok()??;
    String::from_utf8(bytes).ok()
}

/// Store a JSON response body with TTL. Best-effort; errors are swallowed
/// because the cache is purely accelerative — a write failure just means
/// the next read goes to PG.
pub fn set_json(kevy: &KevyStore, key: &str, body: &str, ttl_secs: u64) {
    let _ = kevy.set_with_ttl(
        key.as_bytes(),
        body.as_bytes(),
        std::time::Duration::from_secs(ttl_secs),
    );
}

/// Invalidate every cached read for a user. Uses the embed store's
/// collect_keys (Redis KEYS-style glob) to gather matching keys, then
/// batch DEL them. collect_keys is O(n) over the keyspace but kevy is
/// in-process so it's microseconds for typical mailrs key counts.
pub fn bust_user(kevy: &KevyStore, user: &str) {
    let patterns = [
        format!("{}:{}:*", PREFIX_LIST, user),
        format!("{}:{}:*", PREFIX_CATS, user),
        format!("{}:{}:*", PREFIX_ACTION, user),
    ];
    for pat in &patterns {
        let keys: Vec<Vec<u8>> = kevy.collect_keys(Some(pat.as_bytes()), None);
        if !keys.is_empty() {
            let refs: Vec<&[u8]> = keys.iter().map(|k| k.as_slice()).collect();
            let _ = kevy.del(&refs);
        }
    }
}

/// Bust list-level caches + a specific thread. Use this after any
/// per-thread mutation (read/unread/star/etc) so the list refreshes its
/// aggregate counts and the thread refetches with the new flags.
pub fn bust_thread(kevy: &KevyStore, user: &str, thread_id: &str) {
    let tk = thread_key(user, thread_id);
    let _ = kevy.del(&[tk.as_bytes()]);
    bust_user(kevy, user);
}

/// Invalidate every conversation cache across every user. Returns the
/// number of keys deleted. Used by the admin flush endpoint after a
/// backend wire-schema change so stale cached JSON from the previous
/// version stops being served — the alternative is letting each user
/// trip the deploy-cache-stale race per-thread.
pub fn bust_all_conversations(kevy: &KevyStore) -> usize {
    let patterns = [
        format!("{}:*", PREFIX_THREAD),
        format!("{}:*", PREFIX_LIST),
        format!("{}:*", PREFIX_CATS),
        format!("{}:*", PREFIX_ACTION),
    ];
    let mut deleted = 0;
    for pat in &patterns {
        let keys: Vec<Vec<u8>> = kevy.collect_keys(Some(pat.as_bytes()), None);
        if !keys.is_empty() {
            let refs: Vec<&[u8]> = keys.iter().map(|k| k.as_slice()).collect();
            if let Ok(n) = kevy.del(&refs) {
                deleted += n;
            }
        }
    }
    deleted
}

#[cfg(test)]
mod tests {
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
}
