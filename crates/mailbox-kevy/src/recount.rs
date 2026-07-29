//! Rebuild thread counters from the messages they summarise.
//!
//! `count` / `unread_count` / `sent_count` are hand-incremented on the
//! arrival path while the per-thread message index is written
//! separately and keyed by message-id. The index dedupes; the counters
//! do not. So a message delivered to two local mailboxes — one send
//! from `devops@golia.jp` to `lihao@golia.jp`, both accounts on this
//! server — runs the arrival path twice against **one** row
//! (`mailrs:thread:<tid>` has no user segment) and leaves `count=2`
//! next to an index holding one message. The second arrival is the
//! sender's own copy, so it also trips `is_own` and adds a
//! `sent_count` that no Sent record backs.
//!
//! 14 of one account's 400 most recent threads carried that on prod.
//!
//! The repair is to stop trusting the counter and ask the messages,
//! which `recount_from_messages` already did for rethread. Nothing
//! here is new logic; it is the existing derivation pointed at the
//! stored aggregate.
//!
//! **This cannot make a shared thread right for both parties.** The
//! row is global, so a thread with two local participants can hold one
//! user's numbers or the other's, and the last sweep wins. Those
//! threads are counted and reported rather than quietly settled — the
//! fix is per-user rows, in
//! `.claude/rfcs/20260730-per-user-thread-state.md`.

use std::io;

use super::KevyMailboxStore;
use super::keys;

/// What one sweep segment saw.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RecountReport {
    pub scanned: u64,
    /// Rows whose stored counters disagreed with their messages.
    pub repaired: u64,
    /// Threads with more than one local participant, where a global row
    /// cannot represent both. Included in `scanned`, and repaired to
    /// the sweeping user's view.
    pub shared: u64,
}

impl KevyMailboxStore {
    /// Recount one (user, thread) and write it back only when it
    /// differs. Returns whether anything was written.
    pub fn repair_thread_counts(&self, user: &str, tid: &str) -> io::Result<bool> {
        let Some(mut row) = self.get_thread(tid)? else {
            return Ok(false);
        };
        let Some((count, unread, sent)) = self.recount_from_messages(user, tid)? else {
            // No messages to count from — leave the row alone rather
            // than zeroing a thread whose index has not been built yet.
            return Ok(false);
        };
        if row.count == count && row.unread_count == unread && row.sent_count == sent {
            return Ok(false);
        }
        row.count = count;
        row.unread_count = unread;
        row.sent_count = sent;
        self.upsert_thread(user, &row)?;
        Ok(true)
    }

    /// Sweep a page of `user`'s threads. Paginated the same way as
    /// `backfill_thread_user`, and idempotent: a second pass over
    /// converged data writes nothing and reports `repaired == 0`.
    pub fn backfill_thread_counts(
        &self,
        user: &str,
        offset: i64,
        limit: i64,
    ) -> io::Result<RecountReport> {
        let prefix = keys::thread_user(user, "");
        let mut ids: Vec<String> = self
            .store()
            .keys(Some(format!("{prefix}*").as_bytes()), None)
            .into_iter()
            .filter_map(|k| {
                let k = String::from_utf8(k).ok()?;
                k.get(prefix.len()..).map(str::to_string)
            })
            .collect();
        ids.sort();

        let mut report = RecountReport::default();
        for tid in ids
            .into_iter()
            .skip(offset.max(0) as usize)
            .take(limit.max(0) as usize)
        {
            report.scanned += 1;
            if self.thread_has_multiple_local_participants(&tid)? {
                report.shared += 1;
            }
            if self.repair_thread_counts(user, &tid)? {
                report.repaired += 1;
            }
        }
        Ok(report)
    }

    /// Whether more than one account on this server holds `tid`.
    ///
    /// Counts membership rows rather than parsing `senders_csv`: the
    /// rows are what actually exist, and a sender string can name an
    /// address that never had mail delivered to it.
    fn thread_has_multiple_local_participants(&self, tid: &str) -> io::Result<bool> {
        // `keys` takes a glob; the bare prefix matches the prefix
        // itself and nothing else.
        let pattern = format!("{}*", String::from_utf8_lossy(keys::THREAD_USER_PREFIX));
        let suffix = format!(":{tid}");
        let n = self
            .store()
            .keys(Some(pattern.as_bytes()), None)
            .into_iter()
            .filter(|k| String::from_utf8_lossy(k).ends_with(&suffix))
            .count();
        Ok(n > 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MessageArrival;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ))
    }

    fn arrival<'a>(
        tid: &'a str,
        user: &'a str,
        sender: &'a str,
        is_own: bool,
    ) -> MessageArrival<'a> {
        MessageArrival {
            thread_id: tid,
            user,
            subject: "Subj",
            senders_csv: sender,
            latest_date: 100,
            latest_preview: "preview",
            category: "inbox",
            unread: !is_own,
            is_own,
        }
    }

    fn wire(mid: &str, sender: &str, seen: bool) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "message_id": mid,
            "sender": sender,
            "flags": if seen { 1 } else { 0 },
            "internal_date": 100,
        }))
        .unwrap()
    }

    /// The reported case: one message from a local sender, delivered to
    /// both mailboxes, counted twice on one row.
    #[test]
    fn a_double_counted_thread_is_repaired_to_what_the_messages_say() {
        let s = store();
        s.record_message_arrival(&arrival("t1", "lihao@x.com", "devops@x.com", false))
            .unwrap();
        s.record_message_arrival(&arrival("t1", "devops@x.com", "devops@x.com", true))
            .unwrap();
        s.upsert_message("t1", "msg-1", 100, &wire("msg-1", "devops@x.com", true))
            .unwrap();

        let before = s.get_thread("t1").unwrap().unwrap();
        assert_eq!(
            (before.count, before.sent_count),
            (2, 1),
            "the bug as stored"
        );

        assert!(s.repair_thread_counts("lihao@x.com", "t1").unwrap());

        let after = s.get_thread("t1").unwrap().unwrap();
        assert_eq!(after.count, 1, "one message in the index, one in the count");
        assert_eq!(after.sent_count, 0, "lihao sent nothing here");
    }

    #[test]
    fn a_converged_thread_is_left_alone() {
        let s = store();
        s.record_message_arrival(&arrival("t1", "u@x.com", "alice@y.com", false))
            .unwrap();
        s.upsert_message("t1", "msg-1", 100, &wire("msg-1", "alice@y.com", false))
            .unwrap();

        // A single inbound message is already counted correctly, so the
        // first pass has nothing to do either — and neither does the
        // second. Writing on a converged row is what
        // `periodic-work-must-converge` forbids.
        assert!(!s.repair_thread_counts("u@x.com", "t1").unwrap());
        assert!(!s.repair_thread_counts("u@x.com", "t1").unwrap());

        let row = s.get_thread("t1").unwrap().unwrap();
        assert_eq!((row.count, row.unread_count, row.sent_count), (1, 1, 0));
    }

    #[test]
    fn a_thread_with_no_indexed_messages_is_not_zeroed() {
        let s = store();
        s.record_message_arrival(&arrival("t1", "u@x.com", "alice@y.com", false))
            .unwrap();

        assert!(!s.repair_thread_counts("u@x.com", "t1").unwrap());
        assert_eq!(s.get_thread("t1").unwrap().unwrap().count, 1);
    }

    #[test]
    fn the_sweep_reports_threads_it_cannot_settle_for_both_parties() {
        let s = store();
        s.record_message_arrival(&arrival("t1", "lihao@x.com", "devops@x.com", false))
            .unwrap();
        s.record_message_arrival(&arrival("t1", "devops@x.com", "devops@x.com", true))
            .unwrap();
        s.upsert_message("t1", "msg-1", 100, &wire("msg-1", "devops@x.com", true))
            .unwrap();

        let r = s.backfill_thread_counts("lihao@x.com", 0, 100).unwrap();
        assert_eq!(r.scanned, 1);
        assert_eq!(r.repaired, 1);
        assert_eq!(r.shared, 1, "two local participants, one global row");
    }
}
