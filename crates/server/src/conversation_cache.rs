// Cache-aside Valkey layer for the hot conversation read endpoints.
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

use redis::AsyncCommands;
use redis::aio::ConnectionManager;

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

/// Get a cached JSON response body. None on miss / connection error.
pub async fn get_json(
    valkey: &ConnectionManager,
    key: &str,
) -> Option<String> {
    let mut conn = valkey.clone();
    conn.get(key).await.ok()
}

/// Store a JSON response body with TTL. Best-effort; errors are swallowed
/// because the cache is purely accelerative — a write failure just means
/// the next read goes to PG.
pub async fn set_json(
    valkey: &ConnectionManager,
    key: &str,
    body: &str,
    ttl_secs: u64,
) {
    let mut conn = valkey.clone();
    let _: Result<(), _> = conn.set_ex::<_, _, ()>(key, body, ttl_secs).await;
}

/// Invalidate every cached read for a user. Walks the keyspace in batches
/// via SCAN+DEL rather than the latency-cliff KEYS command.
pub async fn bust_user(valkey: &ConnectionManager, user: &str) {
    let patterns = [
        format!("{}:{}:*", PREFIX_LIST, user),
        format!("{}:{}:*", PREFIX_CATS, user),
        format!("{}:{}:*", PREFIX_ACTION, user),
    ];
    let mut conn = valkey.clone();
    for pat in &patterns {
        let mut cursor: u64 = 0;
        loop {
            let res: redis::RedisResult<(u64, Vec<String>)> = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(pat)
                .arg("COUNT")
                .arg(200)
                .query_async(&mut conn)
                .await;
            let (next, keys) = match res {
                Ok(v) => v,
                Err(_) => break,
            };
            if !keys.is_empty() {
                let _: redis::RedisResult<()> =
                    redis::cmd("DEL").arg(&keys).query_async(&mut conn).await;
            }
            if next == 0 {
                break;
            }
            cursor = next;
        }
    }
}

/// Bust list-level caches + a specific thread. Use this after any
/// per-thread mutation (read/unread/star/etc) so the list refreshes its
/// aggregate counts and the thread refetches with the new flags.
pub async fn bust_thread(
    valkey: &ConnectionManager,
    user: &str,
    thread_id: &str,
) {
    let mut conn = valkey.clone();
    let _: redis::RedisResult<()> = conn.del::<_, ()>(thread_key(user, thread_id)).await;
    bust_user(valkey, user).await;
}

#[cfg(test)]
#[path = "conversation_cache_tests.rs"]
mod tests;
