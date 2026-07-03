//! `mailrs-fastcore-backfill-sent` — rebuild the
//! `mailrs:user:<u>:threads:sent` index by inspecting each thread's
//! `senders_csv` field and matching against the user's email.
//!
//! A thread is "sent" iff at least one message in the senders_csv came
//! from the user themselves — that is, the local-part or full address
//! shows up in the csv. This is more reliable than the pg-dump-provided
//! `sent_count` field, which comes from a monolith SQL aggregate that
//! tracks all "outbound direction" messages regardless of author.
//!
//! Idempotent — running this after mailer activity re-tightens the
//! index against the freshest senders_csv values.

use kevy_embedded::{Config, Store};
use mailrs_mailbox_kevy::{KevyMailboxStore, keys};
use std::sync::Arc;

/// True when any comma-separated token in `senders_csv` refers to
/// `user`. Match both `local@domain` and just the local-part.
fn contains_user(senders_csv: &str, user: &str) -> bool {
    let user_lc = user.to_lowercase();
    let user_local = user.split_once('@').map(|(l, _)| l.to_lowercase());
    for token in senders_csv.split(',') {
        let t = token.trim().to_lowercase();
        if t.contains(&user_lc) {
            return true;
        }
        if let Some(local) = &user_local {
            // "doracawl <notifications@github.com>" style — extract
            // display name before "<" and compare only if it's an
            // address (contains @) matching user.
            if let Some(lt) = t.find('<')
                && let Some(gt) = t.rfind('>')
                && gt > lt
            {
                let inside = &t[lt + 1..gt];
                if inside.contains(&user_lc) {
                    return true;
                }
                let _ = local; // don't loosen match to bare local-part —
                // "doracawl" collides with too many github users.
            }
        }
    }
    false
}

fn main() {
    let kevy_dir =
        std::env::var("MAILRS_KEVY_DATA_DIR").unwrap_or_else(|_| "/data/kevy-fastcore".to_string());
    let cfg = Config::default().with_persist(&kevy_dir);
    let store = Arc::new(Store::open(cfg).expect("open kevy store"));
    let mailbox = KevyMailboxStore::new(store.clone());

    let users = mailbox
        .list_account_addresses()
        .expect("list_account_addresses");
    let mut total_added = 0u64;
    for user in &users {
        let activity_key = keys::user_threads_by_activity(user);
        let n = store.zcard(activity_key.as_bytes()).unwrap_or(0);
        if n == 0 {
            continue;
        }
        eprintln!("user={user} threads_in_activity={n}");
        let entries = store
            .zrevrange(activity_key.as_bytes(), 0, (n as i64) - 1)
            .expect("zrevrange");
        let sent_key = keys::user_threads_sent(user);
        // Rebuild from scratch: DEL the index first so removed threads
        // (or ones that used to match under the stale predicate) drop out.
        store.del(&[sent_key.as_bytes()]).ok();
        let mut inserted = 0;
        for (tid_bytes, score) in entries {
            let Ok(tid) = std::str::from_utf8(&tid_bytes) else {
                continue;
            };
            let hkey = keys::thread(tid);
            let senders_csv = match store.hget(hkey.as_bytes(), b"senders_csv") {
                Ok(Some(bytes)) => String::from_utf8_lossy(&bytes).to_string(),
                _ => continue,
            };
            if contains_user(&senders_csv, user) {
                store
                    .zadd(sent_key.as_bytes(), &[(score, tid.as_bytes())])
                    .expect("zadd sent");
                inserted += 1;
            }
        }
        eprintln!("  user={user} sent_index_size={inserted}");
        total_added += inserted;
    }
    eprintln!(
        "done: sent_threads_indexed={total_added} across {} users",
        users.len()
    );
}
