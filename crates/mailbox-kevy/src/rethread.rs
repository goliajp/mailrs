//! Thread-merge + `Message-ID → thread_id` index (v2.9.5 threading fix).
//!
//! Threading fragmented because the write paths derived thread ids from
//! three inconsistent rules and nothing recorded which thread a given
//! RFC 5322 `Message-ID:` landed in — a reply carrying `In-Reply-To:
//! <our-msgid>` could not find its conversation and opened a new one.
//! This module adds the reconciliation index and the merge primitive the
//! rethread backfill uses to heal existing fragments.

use std::io;

use crate::KevyMailboxStore;
use crate::keys;

impl KevyMailboxStore {
    /// Record which thread a `Message-ID:` belongs to. Overwrites — the
    /// latest write wins, which is correct after a merge re-points ids.
    pub fn set_thread_for_message_id(&self, user: &str, mid: &str, tid: &str) -> io::Result<()> {
        let key = keys::message_by_message_id(user, mid);
        self.store().set(key.as_bytes(), tid.as_bytes())?;
        Ok(())
    }

    /// Resolve a `Message-ID:` to the thread it was indexed under.
    pub fn thread_for_message_id(&self, user: &str, mid: &str) -> io::Result<Option<String>> {
        let key = keys::message_by_message_id(user, mid);
        let out = self.store().get(key.as_bytes())?;
        Ok(out.and_then(|v| String::from_utf8(v).ok()))
    }

    /// Merge thread `from` into thread `into` for `user`: move every
    /// message (zset member + blob thread_id field), combine the two
    /// aggregates, rebuild `into`'s index memberships, and drop `from`
    /// from every index. Returns how many messages moved. Idempotent —
    /// re-running with an already-merged `from` is a no-op.
    pub fn merge_thread_into(&self, user: &str, from: &str, into: &str) -> io::Result<usize> {
        if from == into {
            return Ok(0);
        }
        let store = self.store();
        let from_msgs_key = keys::thread_messages(from);
        let into_msgs_key = keys::thread_messages(into);

        // 1. move messages: re-point each blob's thread_id and move the
        //    zset membership (keeping the internal_date score).
        let members = store.zrange(from_msgs_key.as_bytes(), 0, -1)?;
        let mut moved = 0usize;
        for (mid_bytes, score) in &members {
            let Ok(mid) = std::str::from_utf8(mid_bytes) else {
                continue;
            };
            let blob_key = keys::message_blob(mid);
            if let Some(bytes) = store.get(blob_key.as_bytes())?
                && let Ok(mut wire) = serde_json::from_slice::<serde_json::Value>(&bytes)
            {
                wire["thread_id"] = serde_json::Value::String(into.to_string());
                if let Ok(payload) = serde_json::to_vec(&wire) {
                    store.set(blob_key.as_bytes(), &payload)?;
                }
            }
            store.zadd(into_msgs_key.as_bytes(), &[(*score, mid_bytes.as_slice())])?;
            self.set_thread_for_message_id(user, mid, into)?;
            moved += 1;
        }
        store.del(&[from_msgs_key.as_bytes()])?;

        // 2. combine the aggregates. `from` may have no hash (already
        //    merged) — then only the zset move above mattered.
        let from_row = self.get_thread(from)?;
        let into_row = self.get_thread(into)?;
        let merged = match (into_row, from_row) {
            (Some(a), Some(b)) => Some(combine_rows(into, a, b)),
            (Some(a), None) => Some(a),
            (None, Some(mut b)) => {
                b.thread_id = into.to_string();
                Some(b)
            }
            (None, None) => None,
        };

        // 3. drop `from` from every per-user index + its hash, then
        //    rebuild `into`'s memberships from the merged aggregate —
        //    with the counters recounted from the actual message flags.
        //    Naively summing the two hashes' unread_count resurrected
        //    stale pre-migration values that were never in the unread
        //    index (2026-07-17: years-old mail flooded the Unread tab
        //    after the first prod rethread).
        self.delete_thread(user, from)?;
        if let Some(mut row) = merged {
            if let Some((count, unread, sent)) = self.recount_from_messages(user, into)? {
                row.count = count;
                row.unread_count = unread;
                row.sent_count = sent;
            }
            self.upsert_thread(user, &row)?;
        }
        Ok(moved)
    }

    /// Recompute (count, unread_count, sent_count) for a thread from its
    /// per-message wires: unread = messages without the \Seen flag that
    /// the user didn't send; sent = messages the user sent. `None` when
    /// the thread has no messages to count (keep the hash values).
    fn recount_from_messages(
        &self,
        user: &str,
        tid: &str,
    ) -> io::Result<Option<(i64, i64, i64)>> {
        let blobs = self.list_thread_messages(tid)?;
        if blobs.is_empty() {
            return Ok(None);
        }
        let mut count = 0i64;
        let mut unread = 0i64;
        let mut sent = 0i64;
        for b in &blobs {
            let Ok(w) = serde_json::from_slice::<serde_json::Value>(b) else {
                continue;
            };
            count += 1;
            let seen = w["flags"].as_u64().unwrap_or(0) & 1 != 0;
            let sender = w["sender"].as_str().unwrap_or("");
            let is_own = crate::thread_row::senders_csv_contains_user(sender, user);
            if is_own {
                sent += 1;
            } else if !seen {
                unread += 1;
            }
        }
        Ok(Some((count, unread, sent)))
    }
}

fn combine_rows(
    into_tid: &str,
    a: crate::thread_row::ThreadRow,
    b: crate::thread_row::ThreadRow,
) -> crate::thread_row::ThreadRow {
    // the fresher side supplies the display fields
    let (latest, older) = if a.latest_date >= b.latest_date {
        (a.clone(), b.clone())
    } else {
        (b.clone(), a.clone())
    };
    let mut senders: Vec<String> = Vec::new();
    for part in a.senders_csv.split(',').chain(b.senders_csv.split(',')) {
        let p = part.trim();
        if !p.is_empty() && !senders.iter().any(|s| s.eq_ignore_ascii_case(p)) {
            senders.push(p.to_string());
        }
    }
    let mut importance_score = a.importance_score;
    if b.importance_score > importance_score {
        importance_score = b.importance_score;
    }
    crate::thread_row::ThreadRow {
        thread_id: into_tid.to_string(),
        subject: latest.subject,
        senders_csv: senders.join(","),
        count: a.count + b.count,
        unread_count: a.unread_count + b.unread_count,
        latest_date: latest.latest_date,
        latest_preview: latest.latest_preview,
        category: latest.category,
        importance_level: older.importance_level,
        importance_score,
        requires_action: a.requires_action || b.requires_action,
        pinned: a.pinned || b.pinned,
        archived: a.archived && b.archived,
        has_action: a.has_action || b.has_action,
        sent_count: a.sent_count + b.sent_count,
        starred: a.starred || b.starred,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MessageArrival;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = Arc::new(Store::open(Config::default()).expect("open in-memory kevy"));
        KevyMailboxStore::new(s)
    }

    fn arrive(s: &KevyMailboxStore, tid: &str, user: &str, subject: &str, date: i64) {
        s.record_message_arrival(&MessageArrival {
            thread_id: tid,
            user,
            subject,
            senders_csv: "other@x.com",
            latest_date: date,
            latest_preview: "",
            category: "inbox",
            unread: true,
        })
        .unwrap();
    }

    fn put_msg(s: &KevyMailboxStore, tid: &str, mid: &str, date: i64) {
        let wire = serde_json::json!({"message_id": mid, "thread_id": tid, "internal_date": date});
        s.upsert_message(tid, mid, date, &serde_json::to_vec(&wire).unwrap())
            .unwrap();
    }

    #[test]
    fn msgid_index_roundtrip() {
        let s = store();
        assert_eq!(s.thread_for_message_id("u@x", "m1").unwrap(), None);
        s.set_thread_for_message_id("u@x", "m1", "t1").unwrap();
        assert_eq!(
            s.thread_for_message_id("u@x", "m1").unwrap(),
            Some("t1".into())
        );
    }

    #[test]
    fn merge_moves_messages_and_combines_counts() {
        let s = store();
        let u = "u@x.com";
        arrive(&s, "t-old", u, "Re: hello", 100);
        arrive(&s, "t-new", u, "Re: hello", 200);
        put_msg(&s, "t-old", "m1", 100);
        put_msg(&s, "t-new", "m2", 200);

        let moved = s.merge_thread_into(u, "t-old", "t-new").unwrap();
        assert_eq!(moved, 1);

        // t-old gone, t-new has both messages and the summed counts
        assert!(s.get_thread("t-old").unwrap().is_none());
        let row = s.get_thread("t-new").unwrap().expect("merged row");
        assert_eq!(row.count, 2);
        assert_eq!(row.unread_count, 2);
        let msgs = s.list_thread_messages("t-new").unwrap();
        assert_eq!(msgs.len(), 2);
        // moved blob got its thread_id re-pointed
        let m1: serde_json::Value = serde_json::from_slice(&msgs[0]).unwrap();
        assert_eq!(m1["thread_id"], "t-new");
        // msgid index points at the canonical thread
        assert_eq!(
            s.thread_for_message_id(u, "m1").unwrap(),
            Some("t-new".into())
        );
    }

    #[test]
    fn merge_recounts_unread_from_flags_not_stale_hash() {
        let s = store();
        let u = "u@x.com";
        // both fragments carry stale unread_count=1 in the hash, but the
        // actual messages are all \Seen — the merged thread must NOT
        // resurrect the stale unread.
        arrive(&s, "t-old", u, "a", 100);
        arrive(&s, "t-new", u, "a", 200);
        let seen1 = serde_json::json!({"message_id":"m1","thread_id":"t-old","internal_date":100,"flags":1,"sender":"other@x.com"});
        let seen2 = serde_json::json!({"message_id":"m2","thread_id":"t-new","internal_date":200,"flags":1,"sender":"other@x.com"});
        s.upsert_message("t-old", "m1", 100, &serde_json::to_vec(&seen1).unwrap())
            .unwrap();
        s.upsert_message("t-new", "m2", 200, &serde_json::to_vec(&seen2).unwrap())
            .unwrap();

        s.merge_thread_into(u, "t-old", "t-new").unwrap();

        let row = s.get_thread("t-new").unwrap().unwrap();
        assert_eq!(row.count, 2);
        assert_eq!(row.unread_count, 0);
    }

    #[test]
    fn merge_is_idempotent() {
        let s = store();
        let u = "u@x.com";
        arrive(&s, "t-old", u, "a", 100);
        arrive(&s, "t-new", u, "a", 200);
        put_msg(&s, "t-old", "m1", 100);
        put_msg(&s, "t-new", "m2", 200);
        s.merge_thread_into(u, "t-old", "t-new").unwrap();
        let again = s.merge_thread_into(u, "t-old", "t-new").unwrap();
        assert_eq!(again, 0);
        let row = s.get_thread("t-new").unwrap().unwrap();
        assert_eq!(row.count, 2);
    }
}
