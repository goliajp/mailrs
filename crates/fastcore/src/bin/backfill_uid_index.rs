//! `mailrs-fastcore-backfill-uid-index` — reconcile the per-user
//! uid ↔ message_id maps so `/api/mail/messages/{uid}/...` handlers
//! resolve every message unambiguously.
//!
//! History: v1 wrote only the forward `msg_by_uid` map — no reverse
//! entry, no `next_uid` bump — so a migrated store handed out uid=1
//! for the first post-migration delivery and silently overwrote a
//! migrated message's mapping. This version is a full reconcile:
//!
//!   1. walk every user's threads → parse each wire (owner = the
//!      wire's `user_address`, falling back to the walked user)
//!   2. group wires by claimed uid; a uid claimed by >1 message keeps
//!      one deterministic winner (the current forward-map holder if it
//!      is among the claimants, else the oldest `internal_date`)
//!   3. register all winners (both maps + raise `next_uid` past the
//!      max), then re-allocate fresh uids for the losers AND for
//!      uid=0 wires, rewriting their wire payloads in place
//!
//! Idempotent — a second run finds no losers and changes nothing.
//! Run with the owning fastcore STOPPED (embedded kevy dir lock).

use kevy_embedded::{Config, Store};
use mailrs_core_api::method::message::MessageWire;
use mailrs_mailbox_kevy::{KevyMailboxStore, keys};
use std::collections::HashMap;
use std::sync::Arc;

struct WireRef {
    thread_id: String,
    message_id: String,
    internal_date: i64,
    payload: Vec<u8>,
    uid: u32,
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

    // Collect wires per OWNER (wire.user_address) — a thread walked via
    // user A's activity zset can contain messages owned by user B when
    // both parties are local, and uid spaces are strictly per-user.
    let mut by_owner: HashMap<String, HashMap<String, WireRef>> = HashMap::new();
    for user in &users {
        let activity_key = keys::user_threads_by_activity(user);
        let n = store.zcard(activity_key.as_bytes()).unwrap_or(0);
        if n == 0 {
            continue;
        }
        let entries = store
            .zrevrange(activity_key.as_bytes(), 0, (n as i64) - 1)
            .expect("zrevrange activity");
        for (tid_bytes, _score) in entries {
            let Ok(tid) = std::str::from_utf8(&tid_bytes) else {
                continue;
            };
            for payload in mailbox.list_thread_messages(tid).unwrap_or_default() {
                let Ok(wire) = serde_json::from_slice::<MessageWire>(&payload) else {
                    continue;
                };
                let owner = if wire.user_address.is_empty() {
                    user.clone()
                } else {
                    wire.user_address.clone()
                };
                by_owner
                    .entry(owner)
                    .or_default()
                    .entry(wire.message_id.clone())
                    .or_insert(WireRef {
                        thread_id: tid.to_string(),
                        message_id: wire.message_id,
                        internal_date: wire.internal_date,
                        payload,
                        uid: wire.uid,
                    });
            }
        }
    }

    let mut total_registered = 0u64;
    let mut total_reallocated = 0u64;
    for (owner, wires) in &by_owner {
        // group claimants per uid
        let mut claims: HashMap<u32, Vec<&WireRef>> = HashMap::new();
        for w in wires.values() {
            claims.entry(w.uid).or_default().push(w);
        }
        let fwd_key = keys::user_msg_by_uid(owner);
        let mut winners: Vec<&WireRef> = Vec::new();
        let mut losers: Vec<&WireRef> = Vec::new();
        for (uid, mut cs) in claims {
            if uid == 0 {
                losers.extend(cs);
                continue;
            }
            if cs.len() == 1 {
                winners.push(cs[0]);
                continue;
            }
            // collision: prefer whatever the forward map currently
            // resolves (keeps URLs the UI already handed out working),
            // else the oldest message
            let current_holder = store
                .hget(fwd_key.as_bytes(), uid.to_string().as_bytes())
                .ok()
                .flatten()
                .and_then(|b| String::from_utf8(b).ok());
            cs.sort_by_key(|w| w.internal_date);
            let win_idx = current_holder
                .as_deref()
                .and_then(|mid| cs.iter().position(|w| w.message_id == mid))
                .unwrap_or(0);
            eprintln!(
                "  owner={owner} uid={uid} collision x{} — keeping {}",
                cs.len(),
                cs[win_idx].message_id
            );
            for (i, w) in cs.into_iter().enumerate() {
                if i == win_idx {
                    winners.push(w);
                } else {
                    losers.push(w);
                }
            }
        }
        // 1. register winners — raises next_uid past the max claimed uid
        for w in &winners {
            if mailbox.register_uid(owner, w.uid, &w.message_id).is_ok() {
                total_registered += 1;
            }
        }
        // 2. re-allocate losers + uid=0 wires with fresh (now safe) uids.
        //    Drop the loser's reverse entry first — allocate_uid is
        //    idempotent per message_id and would hand the colliding uid
        //    right back otherwise.
        let rev_key = keys::user_uid_by_mid(owner);
        for w in &losers {
            let _ = store.hdel(rev_key.as_bytes(), &[w.message_id.as_bytes()]);
            let Ok(new_uid) = mailbox.allocate_uid(owner, &w.message_id) else {
                continue;
            };
            let Ok(mut wire) = serde_json::from_slice::<MessageWire>(&w.payload) else {
                continue;
            };
            wire.uid = new_uid;
            let Ok(new_payload) = serde_json::to_vec(&wire) else {
                continue;
            };
            let _ =
                mailbox.upsert_message(&w.thread_id, &w.message_id, w.internal_date, &new_payload);
            total_reallocated += 1;
        }
        if !losers.is_empty() {
            eprintln!(
                "  owner={owner} registered={} reallocated={}",
                winners.len(),
                losers.len()
            );
        }
    }
    eprintln!(
        "done: registered={total_registered} reallocated={total_reallocated} across {} owners",
        by_owner.len()
    );
}
