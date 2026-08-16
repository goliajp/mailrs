//! `deliver_message` — single call that joins the thread-aggregate
//! update with per-message blob storage.
//!
//! Phase 7.12. The real receiver-split path is:
//!   1. parse the incoming message (subject, sender, date, body)
//!   2. resolve thread_id (via existing threading logic)
//!   3. call `KevyMailboxStore::deliver_message(&arrival, mid, &blob)`
//!
//! Which atomically:
//!   - updates `mailrs:thread:<tid>` aggregate (hincrby count, hset
//!     latest_*, zadd indexes — via record_message_arrival)
//!   - writes `mailrs:msg:<mid>` blob + zadd's `mailrs:thread:<tid>:messages`
//!     (via upsert_message)
//!
//! Not currently atomic across the two halves (the underlying
//! `Store::atomic<R>` closure can't run zadd-on-the-thread-zset and
//! the per-message string set in the same block via AtomicCtx 1.15.0
//! — same gap reported in
//! .claude/notes/kevy-feedback-atomicctx-zrem-hdel-2026-07-01.md).
//! Either half can succeed independently; the worst case is a
//! sub-millisecond window where the thread row is updated but the
//! blob isn't yet visible. UI re-fetches resolve.

use std::io;

use super::KevyMailboxStore;
use super::message_arrival::MessageArrival;

impl KevyMailboxStore {
    /// Apply a fully-built message arrival to all storage layers in
    /// one call. `payload` is opaque — by convention `mailrs-fastcore`
    /// uses serde-json'd `MessageWire` so webapi gets the same JSON
    /// shape the monolith returns.
    pub fn deliver_message(
        &self,
        arrival: &MessageArrival<'_>,
        message_id: &str,
        payload: &[u8],
        per_user: &crate::UserMessageFacts<'_>,
    ) -> io::Result<()> {
        // **The message row first, the arrival second.**
        //
        // The arrival writes the membership row, and two of that row's
        // declared columns — `sent_only` and `is_sender` — are derived
        // from the aggregate index over these message rows. Recording the
        // arrival first asks the index a question about a message it
        // cannot see yet, so a reply arriving into a thread the user had
        // only sent in was still counted as sent-only and stayed out of
        // their Inbox.
        //
        // The old order worked by accident: `sent_only` came from the
        // counters, and the counters were incremented inside the
        // arrival's own atomic block, so the derivation could not run
        // early. Moving it onto the index removed that coupling and with
        // it the ordering it silently provided.
        self.upsert_user_message(
            arrival.user,
            arrival.thread_id,
            message_id,
            arrival.latest_date,
            payload,
            per_user,
        )?;
        self.record_message_arrival(arrival)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UserMessageFacts;

    /// One user's copy, for tests that only care about the shared half.
    fn test_facts() -> UserMessageFacts<'static> {
        UserMessageFacts {
            blob_ref: "1785000000.M1P1.host",
            uid: 1,
            flags: 0,
            modseq: 1,
        }
    }
    use crate::keys;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        // Reads are served from the declared table, so a test store
        // has to look like a booted one.
        s.ensure_thread_table();
        // The aggregate index that derives the counters, too — without
        // it every count reads zero, which looks exactly like a broken
        // count rather than a store that was never fully booted.
        s.ensure_admin_indexes();
        s
    }

    fn arr<'a>(tid: &'a str, user: &'a str, date: i64) -> MessageArrival<'a> {
        MessageArrival {
            thread_id: tid,
            user,
            subject: "Subj",
            senders_csv: "x@y.z",
            latest_date: date,
            latest_preview: "preview",
            category: "inbox",
            unread: true,
            is_own: false,
        }
    }

    #[test]
    fn deliver_writes_thread_row_and_message_blob() {
        let s = store();
        s.deliver_message(&arr("t1", "u@x.com", 100), "m1", b"blob-1", &test_facts())
            .unwrap();

        // thread row exists, and its counts come from the index — they
        // are per-user, and the shared hash has no user segment.
        assert!(s.get_thread("t1").unwrap().is_some());
        assert_eq!(s.counts_from_index("u@x.com", "t1"), Some((1, 1, 0)));

        // message blob exists at message_blob key
        let blob = s.get_message("m1").unwrap().unwrap();
        assert_eq!(blob, b"blob-1");

        // message_id is in the thread-messages zset with correct score
        let zset = keys::thread_messages("t1");
        assert_eq!(
            s.store_ref().zscore(zset.as_bytes(), b"m1").unwrap(),
            Some(100.0)
        );
    }

    #[test]
    fn two_deliveries_to_same_thread_chain_properly() {
        let s = store();
        s.deliver_message(&arr("t1", "u@x.com", 100), "m1", b"first", &test_facts())
            .unwrap();
        s.deliver_message(&arr("t1", "u@x.com", 200), "m2", b"second", &test_facts())
            .unwrap();

        // thread aggregate bumped
        let row = s.get_thread("t1").unwrap().unwrap();
        assert_eq!(row.latest_date, 200);
        assert_eq!(s.counts_from_index("u@x.com", "t1"), Some((2, 2, 0)));

        // list_thread_messages returns in chronological order
        let blobs = s.thread_messages_for_maintenance("t1").unwrap();
        assert_eq!(blobs, vec![b"first".to_vec(), b"second".to_vec()]);
    }
}
