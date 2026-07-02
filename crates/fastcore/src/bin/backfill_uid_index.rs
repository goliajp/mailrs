//! `mailrs-fastcore-backfill-uid-index` — build the per-user
//! uid → message_id index so `/api/mail/messages/{uid}/...` handlers
//! can resolve the message row without scanning every thread.
//!
//! Walks per-user activity zset → thread messages → parses each JSON
//! MessageWire → HSET `mailrs:user:<u>:msg_by_uid` hash with uid as field.
//! Idempotent — safe to re-run.

use kevy_embedded::{Config, Store};
use mailrs_core_api::method::message::MessageWire;
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
    let mut total_indexed = 0u64;
    for user in &users {
        let activity_key = keys::user_threads_by_activity(user);
        let n = store.zcard(activity_key.as_bytes()).unwrap_or(0);
        if n == 0 {
            continue;
        }
        eprintln!("user={user} threads={n}");
        let entries = store
            .zrevrange(activity_key.as_bytes(), 0, (n as i64) - 1)
            .expect("zrevrange activity");
        let mut indexed = 0;
        for (tid_bytes, _score) in entries {
            let Ok(tid) = std::str::from_utf8(&tid_bytes) else {
                continue;
            };
            let msgs = mailbox.list_thread_messages(tid).unwrap_or_default();
            for payload in msgs {
                let Ok(wire) = serde_json::from_slice::<MessageWire>(&payload) else {
                    continue;
                };
                if mailbox.index_uid(user, wire.uid, &wire.message_id).is_ok() {
                    indexed += 1;
                }
            }
        }
        eprintln!("  user={user} uid_index_entries={indexed}");
        total_indexed += indexed;
    }
    eprintln!(
        "done: total_uid_entries={total_indexed} across {} users",
        users.len()
    );
}
