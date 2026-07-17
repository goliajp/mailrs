//! `mark_seen` — flip a thread from unread → seen.
//!
//! v2 Stage B.1: kevy 3.17 `AtomicCtx` now exposes `zrem` and `hdel`,
//! so the two-op split (hset in atomic + zrem outside) is history.
//! Both ops now run inside a single atomic closure — no millisecond
//! window where the row reads `unread_count = 0` but the has_unread
//! index still lists it.

use std::io;

use super::KevyMailboxStore;
use super::keys;

impl KevyMailboxStore {
    /// Mark `thread_id` as seen for `user` — zero the unread counter
    /// Sweep every unread thread for `user` — walks the
    /// `user:<u>:threads:has_unread` zset and calls `mark_seen` on each.
    /// Returns the number of threads flipped. Idempotent: a second call
    /// with no unread threads returns 0.
    pub fn mark_all_seen(&self, user: &str) -> io::Result<u32> {
        let idx = keys::user_threads_has_unread(user);
        let members = self.store().zrange(idx.as_bytes(), 0, -1)?;
        let mut flipped = 0u32;
        for (tid_bytes, _score) in members {
            let Ok(tid) = std::str::from_utf8(&tid_bytes) else {
                continue;
            };
            // Copy the tid so we don't borrow across the mark_seen call
            // (which reads from other zsets internally).
            let tid = tid.to_string();
            if self.mark_seen(user, &tid)? {
                flipped += 1;
            }
        }
        Ok(flipped)
    }

    /// and drop the row from the `has_unread` index.
    ///
    /// Idempotent: re-applying produces the same state. Returns `true`
    /// if the thread row was found (regardless of whether the unread
    /// count actually flipped); `false` if the row doesn't exist.
    pub fn mark_seen(&self, user: &str, thread_id: &str) -> io::Result<bool> {
        let thread_key = keys::thread(thread_id);
        let idx = keys::user_threads_has_unread(user);
        let found = self.store().atomic(|ctx| {
            let exists = ctx.hexists(thread_key.as_bytes(), b"unread_count")?;
            // Always drop from the has_unread index AND always plant
            // a concrete `unread_count = 0` on the hash. The previous
            // version guarded the hset behind `exists`, so a thread
            // whose hash lacked the field (self-heal-created threads
            // that never went through `record_message_arrival`) had
            // no persistent zero. Any subsequent `hincrby thread:<tid>
            // unread_count 1` would count from 0 → 1 and light the
            // row back up. Writing an explicit zero prevents that.
            ctx.zrem(idx.as_bytes(), &[thread_id.as_bytes()])?;
            ctx.hset(thread_key.as_bytes(), &[(b"unread_count" as &[u8], b"0")])?;
            Ok(exists)
        });
        let exists = found?;
        // Sink the \Seen fact into every per-message wire too. The
        // thread-hash zero above is a cache; the wires are what
        // self-heal recounts and what a rethread merge recounts from —
        // without this, a restart (self-heal) or a merge resurrected
        // already-read mail as unread (2026-07-17).
        let msgs_key = keys::thread_messages(thread_id);
        let members = self.store().zrange(msgs_key.as_bytes(), 0, -1)?;
        for (mid_bytes, _score) in &members {
            let Ok(mid) = std::str::from_utf8(mid_bytes) else {
                continue;
            };
            let blob_key = keys::message_blob(mid);
            if let Some(bytes) = self.store().get(blob_key.as_bytes())?
                && let Ok(mut wire) = serde_json::from_slice::<serde_json::Value>(&bytes)
            {
                let flags = wire["flags"].as_u64().unwrap_or(0);
                if flags & 1 == 0 {
                    wire["flags"] = serde_json::Value::from(flags | 1);
                    if let Ok(payload) = serde_json::to_vec(&wire) {
                        self.store().set(blob_key.as_bytes(), &payload)?;
                    }
                }
            }
        }
        Ok(exists)
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

    fn arr<'a>(tid: &'a str, user: &'a str, unread: bool) -> MessageArrival<'a> {
        MessageArrival {
            thread_id: tid,
            user,
            subject: "Subj",
            senders_csv: "x@y.z",
            latest_date: 100,
            latest_preview: "preview",
            category: "inbox",
            unread,
            is_own: false,
        }
    }

    #[test]
    fn mark_seen_zeros_unread_and_drops_from_index() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u, true)).unwrap();
        let unread_idx = keys::user_threads_has_unread(u);
        assert_eq!(s.store().zcard(unread_idx.as_bytes()).unwrap(), 1);

        let flipped = s.mark_seen(u, "t1").unwrap();
        assert!(flipped);

        let row = s.get_thread("t1").unwrap().unwrap();
        assert_eq!(row.unread_count, 0);
        assert_eq!(s.store().zcard(unread_idx.as_bytes()).unwrap(), 0);
    }

    #[test]
    fn mark_seen_missing_thread_returns_false() {
        let s = store();
        assert!(!s.mark_seen("u@x.com", "nope").unwrap());
    }

    #[test]
    fn mark_seen_is_idempotent() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u, true)).unwrap();
        assert!(s.mark_seen(u, "t1").unwrap());
        assert!(s.mark_seen(u, "t1").unwrap()); // 2nd call OK
        let row = s.get_thread("t1").unwrap().unwrap();
        assert_eq!(row.unread_count, 0);
    }

    #[test]
    fn list_after_mark_seen_excludes_from_has_unread_filter() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("a", u, true)).unwrap();
        s.record_message_arrival(&arr("b", u, true)).unwrap();
        s.mark_seen(u, "a").unwrap();

        let filter = crate::ListThreadsFilter {
            has_unread: true,
            ..Default::default()
        };
        let (rows, total) = s.list_threads_by_activity(u, &filter, 0, 10).unwrap();
        assert_eq!(total, 1);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].thread_id, "b");
    }
}
