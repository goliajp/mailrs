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
    ///
    /// Writes **both** copies: the shared row the read path still uses,
    /// and this user's own counters on the membership row that S4 will
    /// read instead. One repair function for both means they cannot be
    /// repaired to different answers, and it makes the S2 backfill the
    /// same sweep that already exists rather than a second one.
    pub fn repair_thread_counts(&self, user: &str, tid: &str) -> io::Result<bool> {
        let Some(mut row) = self.get_thread(tid)? else {
            return Ok(false);
        };
        let (count, unread, sent) = match self.recount_from_messages(user, tid)? {
            Some(counted) => counted,
            None => {
                // No messages to count from. Zeroing would erase a
                // thread whose index has not been built yet, but
                // leaving the per-user row unwritten is worse: it reads
                // as zero anyway, and once S4 serves counts from it the
                // thread renders as empty. 182 of lihao's threads on
                // prod are in this state — an aggregate row with no
                // message entities behind it — and the S3 shadow report
                // is how they surfaced.
                //
                // The per-user row has to be able to answer for every
                // thread it exists for. When the messages cannot say,
                // the shared row is the only information there is, so
                // carry it over rather than inventing a zero.
                (row.count, row.unread_count, row.sent_count)
            }
        };

        let tu_key = keys::thread_user(user, tid);
        let want: [(&[u8], Vec<u8>); 3] = [
            (b"count", count.to_string().into_bytes()),
            (b"unread_count", unread.to_string().into_bytes()),
            (b"sent_count", sent.to_string().into_bytes()),
        ];
        let per_user_agrees = want.iter().try_fold(true, |acc, (field, value)| {
            let have = self.store().hget(tu_key.as_bytes(), field)?;
            Ok::<bool, io::Error>(acc && have.as_deref() == Some(value.as_slice()))
        })?;

        let shared_agrees =
            row.count == count && row.unread_count == unread && row.sent_count == sent;
        if shared_agrees && per_user_agrees {
            return Ok(false);
        }

        if !per_user_agrees {
            let pairs: Vec<(&[u8], &[u8])> = want.iter().map(|(f, v)| (*f, v.as_slice())).collect();
            self.store().hset(tu_key.as_bytes(), &pairs)?;
        }
        if !shared_agrees {
            row.count = count;
            row.unread_count = unread;
            row.sent_count = sent;
            self.upsert_thread(user, &row)?;
        }
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

        // Resolved once per page, not once per thread.
        let accounts = self.list_account_addresses().unwrap_or_default();
        if accounts.is_empty() {
            // Without the account list every thread looks unshared, and
            // `shared: 0` would read as "checked, found none" when
            // nothing was checked at all. Say so instead.
            return Err(io::Error::other(
                "no accounts registered — cannot tell which threads have \
                 more than one local participant",
            ));
        }

        let mut report = RecountReport::default();
        for tid in ids
            .into_iter()
            .skip(offset.max(0) as usize)
            .take(limit.max(0) as usize)
        {
            report.scanned += 1;
            if self.thread_has_multiple_local_participants(&accounts, &tid)? {
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
    /// Probes one key per account rather than scanning the keyspace.
    /// The first version globbed `mailrs:threaduser:*` and filtered by
    /// suffix, which is a full scan of 30,510 keys **per thread** — 30
    /// million string comparisons for a 1000-row page, and the only
    /// reason the prod sweep crawled. `hot-path-needs-a-plan` is about
    /// exactly this: the predicate looked cheap and had no plan behind
    /// it. Accounts are a single-digit count and the key is exact, so
    /// this is a handful of O(1) lookups.
    ///
    /// Counts membership rows rather than parsing `senders_csv`: the
    /// rows are what actually exist, and a sender string can name an
    /// address that never had mail delivered to it.
    fn thread_has_multiple_local_participants(
        &self,
        accounts: &[String],
        tid: &str,
    ) -> io::Result<bool> {
        let mut seen = 0;
        for account in accounts {
            let key = keys::thread_user(account, tid);
            if self.store().exists(&[key.as_bytes()])? > 0 {
                seen += 1;
                if seen > 1 {
                    return Ok(true);
                }
            }
        }
        Ok(false)
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

    /// The sweep asks the account index which addresses to probe, so a
    /// store used for it has to look like one with accounts on it.
    fn with_accounts(addresses: &[&str]) -> KevyMailboxStore {
        let s = store();
        for a in addresses {
            s.upsert_account(a, r#"{"active":true}"#).unwrap();
        }
        s
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

    /// Every path that writes a counter must write both copies.
    ///
    /// This is the gap that produced the reported bug and the one the
    /// `1↓ 1↑` badge made visible: the arrival path maintained an
    /// aggregate by hand and nothing checked it against the thing it
    /// summarised. Walking each mutation and re-reading both copies is
    /// the check. A new counter writer that forgets the per-user row
    /// fails here rather than on someone's screen.
    #[test]
    fn every_mutation_keeps_both_copies_of_the_counters_in_step() {
        let s = with_accounts(&["u@x.com"]);
        let tid = "t1";
        let user = "u@x.com";

        let check = |label: &str| {
            let shared = s.get_thread(tid).unwrap().unwrap();
            let per_user = |f: &str| {
                s.store()
                    .hget(keys::thread_user(user, tid).as_bytes(), f.as_bytes())
                    .unwrap()
                    .and_then(|v| String::from_utf8(v).ok())
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(-1)
            };
            assert_eq!(
                shared.unread_count,
                per_user("unread_count"),
                "{label}: unread"
            );
            assert_eq!(shared.count, per_user("count"), "{label}: count");
            assert_eq!(shared.sent_count, per_user("sent_count"), "{label}: sent");
        };

        // A single-participant thread is the case where the two copies
        // are expected to agree exactly at every step.
        s.record_message_arrival(&arrival(tid, user, "alice@y.com", false))
            .unwrap();
        check("after arrival");

        s.mark_seen(user, tid).unwrap();
        check("after mark_seen");

        s.mark_unread(user, tid).unwrap();
        check("after mark_unread");

        s.record_message_arrival(&arrival(tid, user, "alice@y.com", false))
            .unwrap();
        check("after a second arrival");

        s.set_starred(user, tid, true).unwrap();
        check("after starring");

        s.set_archived(user, tid, true).unwrap();
        check("after archiving");
    }

    /// Reporting `shared: 0` because the probe had nothing to probe
    /// would be indistinguishable from having checked.
    #[test]
    fn a_sweep_without_an_account_list_fails_rather_than_reporting_zero() {
        let s = store();
        s.record_message_arrival(&arrival("t1", "u@x.com", "alice@y.com", false))
            .unwrap();

        assert!(s.backfill_thread_counts("u@x.com", 0, 10).is_err());
    }

    #[test]
    /// A thread whose aggregate row has no message entities behind it —
    /// 182 of one prod account's threads, found by the S3 shadow report.
    ///
    /// The count cannot be derived, so neither copy may be zeroed. It
    /// also may not be left unwritten: an absent per-user field reads as
    /// zero, so once S4 serves counts from that row the thread would
    /// render empty. The shared value carries over.
    fn a_thread_with_no_indexed_messages_carries_the_shared_count_over() {
        let s = store();
        s.record_message_arrival(&arrival("t1", "u@x.com", "alice@y.com", false))
            .unwrap();
        // Wipe the per-user copy to model a row that predates S1.
        s.store()
            .hdel(
                keys::thread_user("u@x.com", "t1").as_bytes(),
                &[b"count" as &[u8], b"unread_count", b"sent_count"],
            )
            .unwrap();

        assert!(s.repair_thread_counts("u@x.com", "t1").unwrap());

        assert_eq!(
            s.get_thread("t1").unwrap().unwrap().count,
            1,
            "the shared row must not be zeroed"
        );
        let per_user = s
            .store()
            .hget(keys::thread_user("u@x.com", "t1").as_bytes(), b"count")
            .unwrap();
        assert_eq!(
            per_user.as_deref(),
            Some(b"1" as &[u8]),
            "and the per-user row must answer, not stay absent"
        );
    }

    #[test]
    fn the_sweep_reports_threads_it_cannot_settle_for_both_parties() {
        let s = with_accounts(&["lihao@x.com", "devops@x.com"]);
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
