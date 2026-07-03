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
    ) -> io::Result<()> {
        self.record_message_arrival(arrival)?;
        self.upsert_message(arrival.thread_id, message_id, arrival.latest_date, payload)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = Arc::new(Store::open(Config::default()).expect("open in-memory kevy"));
        KevyMailboxStore::new(s)
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
        }
    }

    #[test]
    fn deliver_writes_thread_row_and_message_blob() {
        let s = store();
        s.deliver_message(&arr("t1", "u@x.com", 100), "m1", b"blob-1")
            .unwrap();

        // thread row exists
        let row = s.get_thread("t1").unwrap().unwrap();
        assert_eq!(row.count, 1);
        assert_eq!(row.unread_count, 1);

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
        s.deliver_message(&arr("t1", "u@x.com", 100), "m1", b"first")
            .unwrap();
        s.deliver_message(&arr("t1", "u@x.com", 200), "m2", b"second")
            .unwrap();

        // thread aggregate bumped
        let row = s.get_thread("t1").unwrap().unwrap();
        assert_eq!(row.count, 2);
        assert_eq!(row.unread_count, 2);
        assert_eq!(row.latest_date, 200);

        // list_thread_messages returns in chronological order
        let blobs = s.list_thread_messages("t1").unwrap();
        assert_eq!(blobs, vec![b"first".to_vec(), b"second".to_vec()]);
    }
}
