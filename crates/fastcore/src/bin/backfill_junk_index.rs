//! `mailrs-fastcore-backfill-junk-index` — populate the
//! `mailrs:user:<u>:threads:junk` zset from every user's
//! `mailrs:user:<u>:threads:by_category:spam` and
//! `...by_category:scam` zsets.
//!
//! Ran once per prod after the v2.4.0 Phase 2 cutover so
//! pre-cutover spam-classified threads become visible in the Junk
//! folder tab. Post-cutover arrivals populate `user_threads_junk`
//! directly (see `crates/mailbox-kevy/src/thread_row.rs::upsert_thread`)
//! and do NOT need this tool.
//!
//! Idempotent — running it again after new spam threads arrive
//! silently rewrites the same entries with `ZAggregate::Max` so
//! the latest scores stick.
//!
//! Referenced from the roadmap plan §4.3 (Phase 4 term sweep).
//! Fixes the deferred backfill from v2.4.0 Phase 2 §12.9 (the
//! `scripts/backfill-junk-index.sh` shell wrapper couldn't run
//! because `kevy-cli` is only inside the sidecar image).

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

    let mut total_indexed = 0u64;
    for user in &users {
        // Read every thread from spam + scam category zsets, ZADD
        // into the junk zset. Duplicate members from the two source
        // sets fold onto the same junk entry — the second write
        // just refreshes the score.
        let junk_key = keys::user_threads_junk(user);
        let spam_key = keys::user_threads_by_category(user, "spam");
        let scam_key = keys::user_threads_by_category(user, "scam");
        let mut copied = 0u64;
        for (label, src) in [("spam", &spam_key), ("scam", &scam_key)] {
            let n = store.zcard(src.as_bytes()).unwrap_or(0);
            if n == 0 {
                continue;
            }
            let entries = store
                .zrevrange(src.as_bytes(), 0, (n as i64) - 1)
                .expect("zrevrange");
            for (tid, score) in entries {
                store
                    .zadd(junk_key.as_bytes(), &[(score, tid.as_slice())])
                    .expect("zadd junk");
                copied += 1;
            }
            let _ = label; // only for the eprintln below
        }
        if copied > 0 {
            eprintln!(
                "user={user} junk_backfilled={copied} \
                 (from by_category:spam + by_category:scam)"
            );
            total_indexed += copied;
        }
    }
    eprintln!(
        "done: junk_threads_indexed={total_indexed} across {} users",
        users.len()
    );
}
