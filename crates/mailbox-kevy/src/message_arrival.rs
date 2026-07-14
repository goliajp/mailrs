//! Message-arrival fan-out — the write-path counterpart to
//! `list_threads_by_activity`.
//!
//! Phase 7.7 — single `atomic<R>(closure)` block updates the thread
//! aggregate hash + every index zset the new message touches. Replaces
//! the SQL "INSERT messages; recompute aggregates" pair that prod
//! traces show as the long pole on bulk delivery. All ops queue into
//! the same `AtomicCtx`; commit applies one AOF append + one fsync.

use std::io;

use super::KevyMailboxStore;
use super::keys;

/// What we know about an arriving message at the point we update the
/// thread index. Subject + preview overwrite (latest wins); count and
/// unread bump atomically.
#[derive(Debug, Clone)]
pub struct MessageArrival<'a> {
    pub thread_id: &'a str,
    pub user: &'a str,
    pub subject: &'a str,
    pub senders_csv: &'a str,
    pub latest_date: i64,
    pub latest_preview: &'a str,
    pub category: &'a str,
    /// `true` for an inbound message the recipient hasn't read yet.
    /// `false` for sent-folder writes — bumps `sent_count` instead of
    /// `unread_count`.
    pub unread: bool,
}

impl KevyMailboxStore {
    /// Apply a single message arrival to its thread row + per-user
    /// indexes, all in one atomic block.
    ///
    /// Replaces the 4-statement SQL fanout (INSERT messages; UPDATE
    /// thread set count = count + 1; UPDATE thread set
    /// unread_count = unread_count + 1) with one HSET-overwrite +
    /// 2 HINCRBYs + 3 ZADDs. Conditional has_unread zset toggle uses
    /// the post-increment unread_count read inside the block — no extra
    /// round trip.
    pub fn record_message_arrival(&self, m: &MessageArrival<'_>) -> io::Result<()> {
        let thread_key = keys::thread(m.thread_id);
        let activity = keys::user_threads_by_activity(m.user);
        let cat = keys::user_threads_by_category(m.user, m.category);
        let has_unread = keys::user_threads_has_unread(m.user);
        // v2.8.2 — folder membership on the ARRIVAL path. Prior to this
        // only `upsert_thread` / `set_junk` maintained the inbox/junk
        // zsets, so every thread ingested via this (the main) path was
        // missing from the Inbox folder axis and the UI had to fall
        // back to the mixed by_activity zset.
        let inbox = keys::user_threads_inbox(m.user);
        let junk = keys::user_threads_junk(m.user);
        let is_junk =
            m.category.eq_ignore_ascii_case("spam") || m.category.eq_ignore_ascii_case("scam");

        // Pre-build owned byte buffers — &str → Vec<u8> once, then
        // hand &[u8] refs into the atomic block.
        let subj = m.subject.as_bytes().to_vec();
        let senders = m.senders_csv.as_bytes().to_vec();
        let date_s = m.latest_date.to_string().into_bytes();
        let preview = m.latest_preview.as_bytes().to_vec();
        let category = m.category.as_bytes().to_vec();
        let tid_b = m.thread_id.as_bytes().to_vec();

        self.store().atomic(|ctx| {
            // Overwrite mutable fields with the new message's values.
            let pairs: &[(&[u8], &[u8])] = &[
                (b"subject", &subj),
                (b"senders_csv", &senders),
                (b"latest_date", &date_s),
                (b"latest_preview", &preview),
                (b"category", &category),
            ];
            ctx.hset(thread_key.as_bytes(), pairs)?;

            // Atomic counters.
            let total = ctx.hincrby(thread_key.as_bytes(), b"count", 1)?;
            let new_unread = if m.unread {
                ctx.hincrby(thread_key.as_bytes(), b"unread_count", 1)?
            } else {
                ctx.hincrby(thread_key.as_bytes(), b"sent_count", 1)?;
                // peek current unread; if positive the row still belongs
                // in has_unread regardless of this sent-folder write.
                ctx.hget(thread_key.as_bytes(), b"unread_count")?
                    .and_then(|v| std::str::from_utf8(&v).ok().and_then(|s| s.parse().ok()))
                    .unwrap_or(0i64)
            };
            let sent = ctx
                .hget(thread_key.as_bytes(), b"sent_count")?
                .and_then(|v| std::str::from_utf8(&v).ok().and_then(|s| s.parse().ok()))
                .unwrap_or(0i64);

            // Activity bump (always — every message advances the row).
            ctx.zadd(activity.as_bytes(), &[(m.latest_date as f64, &tid_b)])?;
            // Category index — same score, replaces any earlier
            // category index entry for this tid in this category zset.
            ctx.zadd(cat.as_bytes(), &[(m.latest_date as f64, &tid_b)])?;

            // has_unread: zadd if and only if the post-arrival
            // unread_count > 0. The closure can't zrem yet (1.15
            // AtomicCtx lacks it), so a fast-read flag carries when
            // the toggle has to flip the other way.
            if new_unread > 0 {
                ctx.zadd(has_unread.as_bytes(), &[(m.latest_date as f64, &tid_b)])?;
            }

            // Folder membership (v2.8.2). Same rules as upsert_thread:
            // spam/scam → Junk; anything else with ≥ 1 received message
            // → Inbox. A sent-only thread (total == sent) lives in the
            // Sent axis alone — it must not surface in the Inbox view.
            if is_junk {
                ctx.zadd(junk.as_bytes(), &[(m.latest_date as f64, &tid_b)])?;
                ctx.zrem(inbox.as_bytes(), &[&tid_b])?;
            } else if total > sent {
                ctx.zadd(inbox.as_bytes(), &[(m.latest_date as f64, &tid_b)])?;
                ctx.zrem(junk.as_bytes(), &[&tid_b])?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ListThreadsFilter;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = Arc::new(Store::open(Config::default()).expect("open in-memory kevy"));
        KevyMailboxStore::new(s)
    }

    fn arr<'a>(
        tid: &'a str,
        user: &'a str,
        subject: &'a str,
        latest_date: i64,
        unread: bool,
    ) -> MessageArrival<'a> {
        MessageArrival {
            thread_id: tid,
            user,
            subject,
            senders_csv: "alice@x.com",
            latest_date,
            latest_preview: "preview text",
            category: "inbox",
            unread,
        }
    }

    #[test]
    fn first_arrival_creates_thread_and_indexes() {
        let s = store();
        s.record_message_arrival(&arr("t1", "u@x.com", "Hello", 100, true))
            .unwrap();
        let row = s.get_thread("t1").unwrap().unwrap();
        assert_eq!(row.subject, "Hello");
        assert_eq!(row.count, 1);
        assert_eq!(row.unread_count, 1);
        assert_eq!(row.sent_count, 0);
        assert_eq!(row.latest_date, 100);
        // appears in activity + category + has_unread
        let act = keys::user_threads_by_activity("u@x.com");
        let cat = keys::user_threads_by_category("u@x.com", "inbox");
        let unread = keys::user_threads_has_unread("u@x.com");
        assert_eq!(s.store().zcard(act.as_bytes()).unwrap(), 1);
        assert_eq!(s.store().zcard(cat.as_bytes()).unwrap(), 1);
        assert_eq!(s.store().zcard(unread.as_bytes()).unwrap(), 1);
    }

    #[test]
    fn second_arrival_bumps_count_and_activity() {
        let s = store();
        s.record_message_arrival(&arr("t1", "u@x.com", "First", 100, true))
            .unwrap();
        s.record_message_arrival(&arr("t1", "u@x.com", "Second", 200, true))
            .unwrap();
        let row = s.get_thread("t1").unwrap().unwrap();
        assert_eq!(row.subject, "Second"); // overwrite
        assert_eq!(row.count, 2);
        assert_eq!(row.unread_count, 2);
        assert_eq!(row.latest_date, 200);
        // activity zset score updated
        let act = keys::user_threads_by_activity("u@x.com");
        assert_eq!(
            s.store().zscore(act.as_bytes(), b"t1").unwrap(),
            Some(200.0)
        );
    }

    #[test]
    fn sent_message_bumps_sent_count_not_unread() {
        let s = store();
        s.record_message_arrival(&arr("t1", "u@x.com", "Outgoing", 100, false))
            .unwrap();
        let row = s.get_thread("t1").unwrap().unwrap();
        assert_eq!(row.sent_count, 1);
        assert_eq!(row.unread_count, 0);
        // Without any unread, has_unread index stays empty.
        let unread = keys::user_threads_has_unread("u@x.com");
        assert_eq!(s.store().zcard(unread.as_bytes()).unwrap(), 0);
    }

    #[test]
    fn inbound_arrival_joins_inbox_folder() {
        let s = store();
        s.record_message_arrival(&arr("t1", "u@x.com", "Hi", 100, true))
            .unwrap();
        let inbox = keys::user_threads_inbox("u@x.com");
        let junk = keys::user_threads_junk("u@x.com");
        assert_eq!(
            s.store().zscore(inbox.as_bytes(), b"t1").unwrap(),
            Some(100.0)
        );
        assert_eq!(s.store().zcard(junk.as_bytes()).unwrap(), 0);
    }

    #[test]
    fn spam_arrival_joins_junk_not_inbox() {
        let s = store();
        let mut a = arr("t1", "u@x.com", "V1AGRA", 100, true);
        a.category = "spam";
        s.record_message_arrival(&a).unwrap();
        let inbox = keys::user_threads_inbox("u@x.com");
        let junk = keys::user_threads_junk("u@x.com");
        assert_eq!(s.store().zcard(inbox.as_bytes()).unwrap(), 0);
        assert_eq!(
            s.store().zscore(junk.as_bytes(), b"t1").unwrap(),
            Some(100.0)
        );
    }

    #[test]
    fn sent_only_thread_stays_out_of_inbox() {
        let s = store();
        // Outbound-only write: count == sent_count → Sent axis only.
        s.record_message_arrival(&arr("t1", "u@x.com", "Outgoing", 100, false))
            .unwrap();
        let inbox = keys::user_threads_inbox("u@x.com");
        assert_eq!(s.store().zcard(inbox.as_bytes()).unwrap(), 0);
        // A reply arriving later promotes the thread into Inbox.
        s.record_message_arrival(&arr("t1", "u@x.com", "Re: Outgoing", 200, true))
            .unwrap();
        assert_eq!(
            s.store().zscore(inbox.as_bytes(), b"t1").unwrap(),
            Some(200.0)
        );
    }

    #[test]
    fn list_after_arrivals_returns_newest_first() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u, "old", 100, true))
            .unwrap();
        s.record_message_arrival(&arr("t2", u, "newer", 200, true))
            .unwrap();
        s.record_message_arrival(&arr("t1", u, "newest", 300, true))
            .unwrap();
        let (rows, total) = s
            .list_threads_by_activity(u, &ListThreadsFilter::default(), 0, 10)
            .unwrap();
        assert_eq!(total, 2);
        assert_eq!(rows[0].thread_id, "t1");
        assert_eq!(rows[0].count, 2);
        assert_eq!(rows[1].thread_id, "t2");
    }
}
