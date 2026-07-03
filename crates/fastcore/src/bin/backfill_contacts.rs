//! `mailrs-fastcore-backfill-contacts` — build the
//! `mailrs:user:<u>:contacts` hash from existing kevy data.
//!
//! Reads thread rows from fastcore's EMBEDDED kevy (where the migrated
//! mail lives) and writes the contact index into the shared NETWORK
//! kevy at `MAILRS_KEVY_URL`, matching where webapi's `/api/contacts`
//! handler reads from.

use kevy_embedded::{Config, Store};
use mailrs_mailbox_kevy::{KevyMailboxStore, keys};
use std::sync::Arc;

/// Parse one `senders_csv` value into `(email, display)` tuples.
fn parse_senders(csv: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for token in csv.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
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
    let store = Arc::new(Store::open(cfg).expect("open embedded kevy"));
    let mailbox = KevyMailboxStore::new(store.clone());

    let net_url =
        std::env::var("MAILRS_KEVY_URL").expect("MAILRS_KEVY_URL required (network kevy)");
    let mut net = kevy_client::Connection::open(&net_url).expect("open network kevy");

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
            .expect("zrevrange");
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
        pending.sort_by(|a, b| a.0.cmp(&b.0));
        pending.dedup_by(|a, b| a.0 == b.0);
        let mut inserted = 0;
        for (email, display) in &pending {
            net.hset(
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
