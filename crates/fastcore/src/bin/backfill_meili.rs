//! `mailrs-fastcore-backfill-meili` — index every existing thread into
//! the per-user Meili index so search covers history, not just mail
//! that arrived after the fix (G10).
//!
//! Walks each user's activity zset → thread rows → `index_meili`. Meili
//! upserts by the sanitized `id` primary key, so a re-run is idempotent.
//! Env: MAILRS_KEVY_DATA_DIR (embedded store), MAILRS_MEILI_URL +
//! MAILRS_MEILI_MASTER_KEY (target).

use kevy_embedded::{Config, Store};
use mailrs_mailbox_kevy::{KevyMailboxStore, keys};
use std::sync::Arc;

fn main() {
    let kevy_dir =
        std::env::var("MAILRS_KEVY_DATA_DIR").unwrap_or_else(|_| "/data/kevy-fastcore".to_string());
    if std::env::var("MAILRS_MEILI_URL").is_err() {
        eprintln!("MAILRS_MEILI_URL unset — nothing to backfill into");
        std::process::exit(1);
    }
    let cfg = Config::default().with_persist(&kevy_dir);
    let store = Arc::new(Store::open(cfg).expect("open kevy store"));
    let mailbox = KevyMailboxStore::new(store.clone());

    let users = mailbox
        .list_account_addresses()
        .expect("list_account_addresses");
    let mut total = 0u64;
    for user in &users {
        let activity = keys::user_threads_by_activity(user);
        let n = store.zcard(activity.as_bytes()).unwrap_or(0);
        if n == 0 {
            continue;
        }
        let entries = store
            .zrevrange(activity.as_bytes(), 0, (n as i64) - 1)
            .unwrap_or_default();
        let mut count = 0u64;
        for (tid_bytes, _score) in entries {
            let Ok(tid) = std::str::from_utf8(&tid_bytes) else {
                continue;
            };
            let Ok(Some(row)) = mailbox.get_thread(tid) else {
                continue;
            };
            mailrs_fastcore::live_sync::index_meili(
                user,
                &row.thread_id,
                &row.subject,
                &row.senders_csv,
                &row.latest_preview,
                row.latest_date,
            );
            count += 1;
            // Meili's async task queue can be overwhelmed by a tight
            // firehose; a tiny yield keeps the enqueue rate sane.
            if count.is_multiple_of(500) {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        }
        eprintln!("  user={user} indexed={count}");
        total += count;
    }
    // give the fire-and-forget index threads a moment to flush
    std::thread::sleep(std::time::Duration::from_secs(3));
    eprintln!("done: indexed={total} across {} users", users.len());
}
