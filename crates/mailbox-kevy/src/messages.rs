//! Per-message storage — write a JSON-serialized payload per message
//! + add to the per-thread index zset (score = internal_date).
//!
//! Phase 7.11. The kevy layout:
//!   mailrs:msg:<message_id>           string  — serde-json of any
//!                                                  type the caller
//!                                                  passes in
//!   mailrs:thread:<tid>:messages      zset    — message_id → internal_date
//!
//! The caller picks the JSON shape. `mailrs-fastcore` writes the same
//! `mailrs_core_api::method::message::MessageWire` rows the monolith
//! returns, so webapi's consumer code is unchanged.

use std::io;

use super::KevyMailboxStore;
use super::keys;

impl KevyMailboxStore {
    /// Write `payload` bytes to the message-blob key + zadd to the
    /// thread's message index with score = `internal_date`. `payload`
    /// is opaque — callers usually pass a serde-json'd MessageWire.
    pub fn upsert_message(
        &self,
        thread_id: &str,
        message_id: &str,
        internal_date: i64,
        payload: &[u8],
    ) -> io::Result<()> {
        let blob_key = keys::message_blob(message_id);
        self.store().set(blob_key.as_bytes(), payload)?;
        let zset = keys::thread_messages(thread_id);
        self.store().zadd(
            zset.as_bytes(),
            &[(internal_date as f64, message_id.as_bytes())],
        )?;
        Ok(())
    }

    /// Read message bytes for `message_id`. Returns `None` if the key
    /// is missing (deleted or never written).
    pub fn get_message(&self, message_id: &str) -> io::Result<Option<Vec<u8>>> {
        let key = keys::message_blob(message_id);
        self.store().get(key.as_bytes())
    }

    /// Look up a message by (user, uid) via the per-user uid → message_id
    /// hash. Returns the raw payload bytes (JSON MessageWire) or None
    /// when the uid isn't indexed (or the message was deleted).
    pub fn get_message_by_uid(&self, user: &str, uid: u32) -> io::Result<Option<Vec<u8>>> {
        let idx_key = keys::user_msg_by_uid(user);
        let mid_bytes = self
            .store()
            .hget(idx_key.as_bytes(), uid.to_string().as_bytes())?;
        let Some(mid_bytes) = mid_bytes else {
            return Ok(None);
        };
        let mid = String::from_utf8_lossy(&mid_bytes).to_string();
        self.get_message(&mid)
    }

    /// Populate the per-user uid → message_id index for a single message.
    /// Called from deliver / migrate paths so per-uid lookups are O(1).
    pub fn index_uid(&self, user: &str, uid: u32, message_id: &str) -> io::Result<()> {
        let idx_key = keys::user_msg_by_uid(user);
        self.store().hset(
            idx_key.as_bytes(),
            &[(uid.to_string().as_bytes(), message_id.as_bytes())],
        )?;
        Ok(())
    }

    /// Assign a per-user uid to `message_id` and persist both directions
    /// of the mapping. Idempotent: if the message already has a uid,
    /// the existing value is returned without touching the counter.
    ///
    /// Used by the self-heal path so `/api/mail/messages/{uid}/…`
    /// endpoints (raw source, attachments) can resolve messages that
    /// weren't handed a uid by the monolith migration.
    pub fn allocate_uid(&self, user: &str, message_id: &str) -> io::Result<u32> {
        let rev_key = keys::user_uid_by_mid(user);
        if let Some(existing) = self
            .store()
            .hget(rev_key.as_bytes(), message_id.as_bytes())?
            && let Ok(s) = std::str::from_utf8(&existing)
            && let Ok(uid) = s.parse::<u32>()
        {
            return Ok(uid);
        }
        let counter_key = keys::user_next_uid(user);
        let uid = self.store().incr(counter_key.as_bytes())?;
        let uid = uid.clamp(1, u32::MAX as i64) as u32;
        self.store().hset(
            rev_key.as_bytes(),
            &[(message_id.as_bytes(), uid.to_string().as_bytes())],
        )?;
        self.index_uid(user, uid, message_id)?;
        Ok(uid)
    }

    /// List all messages in `thread_id` in chronological order
    /// (lowest internal_date first). One ZRANGE + N × GET.
    pub fn list_thread_messages(&self, thread_id: &str) -> io::Result<Vec<Vec<u8>>> {
        let zset = keys::thread_messages(thread_id);
        let entries = self.store().zrange(zset.as_bytes(), 0, -1)?;
        let mut out = Vec::with_capacity(entries.len());
        for (mid_bytes, _score) in entries {
            let Ok(mid) = std::str::from_utf8(&mid_bytes) else {
                continue;
            };
            if let Some(bytes) = self.get_message(mid)? {
                out.push(bytes);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = Arc::new(Store::open(Config::default()).expect("open in-memory kevy"));
        KevyMailboxStore::new(s)
    }

    #[test]
    fn upsert_then_get_round_trips() {
        let s = store();
        s.upsert_message("t1", "msg-1", 100, b"payload-1").unwrap();
        let got = s.get_message("msg-1").unwrap().unwrap();
        assert_eq!(got, b"payload-1");
    }

    #[test]
    fn list_returns_chronological_order() {
        let s = store();
        // out-of-order insertion
        s.upsert_message("t1", "msg-2", 200, b"second").unwrap();
        s.upsert_message("t1", "msg-1", 100, b"first").unwrap();
        s.upsert_message("t1", "msg-3", 300, b"third").unwrap();
        let got = s.list_thread_messages("t1").unwrap();
        assert_eq!(
            got,
            vec![b"first".to_vec(), b"second".to_vec(), b"third".to_vec()]
        );
    }

    #[test]
    fn list_empty_thread_returns_empty_vec() {
        let s = store();
        let got = s.list_thread_messages("never-existed").unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn upsert_overwrites_existing_payload() {
        let s = store();
        s.upsert_message("t1", "msg-1", 100, b"v1").unwrap();
        s.upsert_message("t1", "msg-1", 100, b"v2").unwrap();
        let got = s.get_message("msg-1").unwrap().unwrap();
        assert_eq!(got, b"v2");
        // zset member dedup'd by member name — still 1 entry
        let zset = keys::thread_messages("t1");
        assert_eq!(s.store().zcard(zset.as_bytes()).unwrap(), 1);
    }
}
