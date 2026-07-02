//! `mailrs-fastcore-backfill-contacts` — one-time script to build the
//! `mailrs:user:<u>:contacts` hash from existing kevy data.
//!
//! Walks every thread's `senders_csv` field, parses RFC-5322-ish
//! `Name <email@host>` tokens, and inserts one hash entry per unique
//! email. Idempotent — safe to re-run.

use kevy_embedded::{Config, Store};
use mailrs_mailbox_kevy::{KevyMailboxStore, keys};
use std::sync::Arc;

/// Parse a single `senders_csv` value into `(email, display_name)` pairs.
/// Handles: `Foo <foo@bar>` / bare `foo@bar` / lists separated by `,`.
fn parse_senders(csv: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for token in csv.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        // `Name <email>` form.
        if let Some(lt) = token.rfind('<')
            && let Some(gt) = token.rfind('>')
            && gt > lt
        {
            let email = token[lt + 1..gt].trim().to_string();
            if email.contains('@') {
                out.push((email, token.to_string()));
                continue;
            }
        }
        // Bare email.
        if token.contains('@') {
            out.push((token.to_string(), token.to_string()));
        }
    }
    out
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
        eprintln!("user={user} threads={n}");
        let entries = store
            .zrevrange(activity_key.as_bytes(), 0, (n as i64) - 1)
            .expect("zrevrange activity");
        let contacts_key = format!("mailrs:user:{user}:contacts");
        let mut pending: Vec<(String, String)> = Vec::new();
        for (tid_bytes, _score) in entries {
            let Ok(tid) = std::str::from_utf8(&tid_bytes) else {
                continue;
            };
            let hkey = keys::thread(tid);
            let raw = match store.hget(hkey.as_bytes(), b"senders_csv") {
                Ok(Some(bytes)) => String::from_utf8_lossy(&bytes).to_string(),
                _ => continue,
            };
            for (email, display) in parse_senders(&raw) {
                pending.push((email, display));
            }
        }
        // Dedupe by email — keep the last-seen display name.
        pending.sort_by(|a, b| a.0.cmp(&b.0));
        pending.dedup_by(|a, b| a.0 == b.0);
        let mut inserted = 0;
        for (email, display) in &pending {
            store
                .hset(
                    contacts_key.as_bytes(),
                    &[(email.as_bytes(), display.as_bytes())],
                )
                .expect("hset contact");
            inserted += 1;
        }
        eprintln!("  user={user} contacts_inserted={inserted}");
        total_added += inserted;
    }
    eprintln!(
        "done: total_contacts_upserted={total_added} across {} users",
        users.len()
    );
}
