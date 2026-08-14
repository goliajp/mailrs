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
    /// Sweep every unread thread for `user` — reads the unread axis
    /// off the declared table and calls `mark_seen` on each.
    /// Returns the number of threads flipped. Idempotent: a second call
    /// with no unread threads returns 0.
    pub fn mark_all_seen(&self, user: &str) -> io::Result<u32> {
        let members = self.list_thread_ids_by_flag_via_table(user, "unread", 100_000, 0, None)?;
        let mut flipped = 0u32;
        for tid in &members {
            let tid = tid.as_str();
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
            ctx.hset(thread_key.as_bytes(), &[(b"unread_count" as &[u8], b"0")])?;
            // Mirror onto the membership row the table reads from —
            // both the flag the index keys on and the per-user counter
            // the arrival path increments. Zeroing only the shared
            // row's counter would leave this user's count standing
            // after they read the thread (RFC 20260730 S1: every
            // writer of a counter maintains both copies of it).
            ctx.hset(
                keys::thread_user(user, thread_id).as_bytes(),
                &[(b"unread" as &[u8], b"0" as &[u8]), (b"unread_count", b"0")],
            )?;
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

    /// Set `\Seen` on every message `user` holds in `thread_id`, at the
    /// row level, and report the files that are now behind.
    ///
    /// [`mark_seen`](Self::mark_seen) writes the thread's *counter* and
    /// sinks the bit into the shared blob — where, since stage 5 of the
    /// per-user message projection, no read path consults it: the blob's
    /// `flags` is stripped to zero on write and `user_message_view`
    /// overlays this row on top. So a conversation read in the web left
    /// every message in it unread on the row that serves reads, and
    /// unrenamed on disk.
    ///
    /// Returns `(blob_ref, flags)` for each row it changed, so the caller
    /// — which is the only layer that may touch the filesystem — can bring
    /// the names in line. Rows already carrying the bit are not returned:
    /// a second call reports nothing, which is what makes the repair
    /// converge rather than rename the same files every time.
    pub fn mark_thread_messages_seen(
        &self,
        user: &str,
        thread_id: &str,
    ) -> io::Result<Vec<(String, u32)>> {
        let mut behind = Vec::new();
        for mid in self.user_thread_message_ids(user, thread_id)? {
            if let Some((blob_ref, flags)) = self.mark_user_message_seen(user, &mid)?
                && !blob_ref.is_empty()
            {
                behind.push((blob_ref, flags));
            }
        }
        Ok(behind)
    }

    /// Give one user's copy of one message a UID, but only if it has none.
    ///
    /// Returns the row's `blob_ref` when it wrote, `None` when the row is
    /// missing or already carries a UID.
    ///
    /// **The guard is the point.** A UID is a promise to an IMAP client and
    /// overwriting one silently re-points a number a client is holding, so
    /// this can only ever fill a hole. 215 rows on production have `uid: 0`
    /// — imports whose UID was never allocated — and no client has ever
    /// been told a number for them, which is exactly why giving them one
    /// now is safe and giving a different one to anything else is not.
    pub fn set_user_message_uid_if_unset(
        &self,
        user: &str,
        message_id: &str,
        uid: u32,
    ) -> io::Result<Option<String>> {
        if uid == 0 {
            return Ok(None);
        }
        let Some(facts) = self.user_message_facts(user, message_id)? else {
            return Ok(None);
        };
        if facts.uid != 0 {
            return Ok(None);
        }
        self.store().hset(
            keys::user_message(user, message_id).as_bytes(),
            &[(b"uid".as_slice(), uid.to_string().as_bytes())],
        )?;
        Ok(Some(facts.blob_ref))
    }

    /// Set `\Seen` on one user's copy of one message. `None` when the row
    /// does not exist or already carried the bit; otherwise its `blob_ref`
    /// and its new flags.
    ///
    /// Keyed by message id, and deliberately so: the row is, and asking the
    /// uid index for it first is what made the read-state backfill unable
    /// to repair the very rows it was written for. All 215 of them carry
    /// `uid: 0` — imports whose uid was never allocated — so the lookup
    /// returned nothing, the write was skipped, and the counter that had
    /// already been incremented reported the repair as done. Every run.
    pub fn mark_user_message_seen(
        &self,
        user: &str,
        message_id: &str,
    ) -> io::Result<Option<(String, u32)>> {
        use mailrs_mailbox::types::FLAG_SEEN;

        let Some(facts) = self.user_message_facts(user, message_id)? else {
            return Ok(None);
        };
        if facts.flags & FLAG_SEEN != 0 {
            return Ok(None);
        }
        let flags = facts.flags | FLAG_SEEN;
        self.store().hset(
            keys::user_message(user, message_id).as_bytes(),
            &[(b"flags".as_slice(), flags.to_string().as_bytes())],
        )?;
        Ok(Some((facts.blob_ref, flags)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MessageArrival;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    /// A UID may be filled in and never changed.
    ///
    /// 215 rows on production carry `uid: 0` — imports whose UID was never
    /// allocated — and a message with no UID cannot be fetched by one: the
    /// raw view and every attachment download go through it, and the web
    /// client uses it as the timeline's React key, where a repeated zero is
    /// the duplicate-bubble defect of 2026-07-08. Filling it is safe
    /// because no client has ever been given a number for these. Changing
    /// one would be the opposite.
    #[test]
    fn a_uid_is_filled_in_once_and_never_overwritten() {
        let s = store();
        let u = "u@x.com";
        let seed = |mid: &str, uid: u32| {
            s.upsert_user_message(
                u,
                "t1",
                mid,
                100,
                br#"{"message_id":"m"}"#,
                &crate::UserMessageFacts {
                    blob_ref: "f.host",
                    uid,
                    flags: 0,
                    modseq: 1,
                },
            )
            .unwrap();
        };

        seed("no-uid", 0);
        assert_eq!(
            s.set_user_message_uid_if_unset(u, "no-uid", 7).unwrap(),
            Some("f.host".to_string())
        );
        assert_eq!(s.user_message_facts(u, "no-uid").unwrap().unwrap().uid, 7);

        // Idempotent, and refuses to move a promise.
        assert_eq!(
            s.set_user_message_uid_if_unset(u, "no-uid", 9).unwrap(),
            None
        );
        assert_eq!(s.user_message_facts(u, "no-uid").unwrap().unwrap().uid, 7);

        // Nothing to fill, nothing to say.
        seed("has-uid", 4);
        assert_eq!(
            s.set_user_message_uid_if_unset(u, "has-uid", 9).unwrap(),
            None
        );
        assert_eq!(
            s.set_user_message_uid_if_unset(u, "absent", 9).unwrap(),
            None
        );
        assert_eq!(
            s.set_user_message_uid_if_unset(u, "no-uid", 0).unwrap(),
            None
        );
    }

    fn store() -> KevyMailboxStore {
        let s = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        // Reads are served from the declared table, so a test store
        // has to look like a booted one.
        s.ensure_thread_table();
        s
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
    fn mark_seen_zeros_unread_and_drops_from_the_axis() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u, true)).unwrap();
        let unread = || s.count_thread_ids_by_flag_via_table(u, "unread").unwrap();
        assert_eq!(unread(), 1);

        let flipped = s.mark_seen(u, "t1").unwrap();
        assert!(flipped);

        let row = s.get_thread("t1").unwrap().unwrap();
        assert_eq!(row.unread_count, 0);
        assert_eq!(unread(), 0, "the unread axis must drop it");
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
        // the flag axes are served from the declared table
        s.ensure_thread_table();
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
