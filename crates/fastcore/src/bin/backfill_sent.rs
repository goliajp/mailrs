//! `mailrs-fastcore-backfill-sent` — one-time script to build the
//! `mailrs:user:<u>:threads:sent` index from existing kevy data.
//!
//! Iterates the activity zset for every account, HGETs each thread's
//! `sent_count` field, and ZADDs to the sent index when `> 0`. Idempotent
//! — safe to re-run.

use kevy_embedded::{Config, Store};
use mailrs_mailbox_kevy::{KevyMailboxStore, keys};
use std::sync::Arc;

fn main() {
    let kevy_dir =
        std::env::var("MAILRS_KEVY_DATA_DIR").unwrap_or_else(|_| "/data/kevy-fastcore".to_string());
    let cfg = Config::default().with_persist(&kevy_dir);
    let store = Arc::new(Store::open(cfg).expect("open kevy store"));
    let mailbox = KevyMailboxStore::new(store.clone());

    let users = mailbox
        .list_account_addresses()
        .expect("list_account_addresses");
    let mut backfilled = 0u64;
    for user in &users {
        let activity_key = keys::user_threads_by_activity(user);
        let total = store.zcard(activity_key.as_bytes()).unwrap_or(0);
        eprintln!("user={user} threads_in_activity={total}");
        // ZREVRANGE 0..total-1 to walk every thread.
        if total == 0 {
            continue;
        }
        let entries = store
            .zrevrange(activity_key.as_bytes(), 0, (total as i64) - 1)
            .expect("zrevrange");
        let sent_key = keys::user_threads_sent(user);
        for (tid_bytes, score) in entries {
            let Ok(tid) = std::str::from_utf8(&tid_bytes) else {
                continue;
            };
            let hkey = keys::thread(tid);
            let sc = match store.hget(hkey.as_bytes(), b"sent_count") {
                Ok(Some(bytes)) => String::from_utf8_lossy(&bytes).parse::<i64>().unwrap_or(0),
                _ => 0,
            };
            if sc > 0 {
                store
                    .zadd(sent_key.as_bytes(), &[(score, tid.as_bytes())])
                    .expect("zadd sent");
                backfilled += 1;
            }
        }
        let sent_total = store.zcard(sent_key.as_bytes()).unwrap_or(0);
        eprintln!("  user={user} sent_index_size={sent_total}");
    }
    eprintln!(
        "done: backfilled {backfilled} sent-index entries across {} users",
        users.len()
    );
}
