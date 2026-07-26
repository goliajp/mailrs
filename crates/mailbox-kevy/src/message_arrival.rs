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
    /// Already-read inbound messages (self-heal of a \Seen file) pass
    /// `false` here with `is_own: false`.
    pub unread: bool,
    /// `true` when the user sent this message (sent-folder mirror) —
    /// bumps `sent_count`, and deliberately does NOT advance the
    /// thread's display fields or its position: replying must not
    /// re-date the Inbox row to the user's own send time (2026-07-18).
    pub is_own: bool,
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
        // Pre-build owned byte buffers — &str → Vec<u8> once, then
        // hand &[u8] refs into the atomic block.
        let subj = m.subject.as_bytes().to_vec();
        let date_s = m.latest_date.to_string().into_bytes();
        let preview = m.latest_preview.as_bytes().to_vec();
        let category = m.category.as_bytes().to_vec();

        self.store()
            .atomic(|ctx| {
                // senders_csv is the participant UNION, not "latest sender" —
                // blindly overwriting meant a user's own reply erased every
                // other participant and the Inbox row flipped to "Me"
                // (2026-07-18). Merge case-insensitively, newest appended.
                let merged_senders: Vec<u8> = {
                    let existing = ctx
                        .hget(thread_key.as_bytes(), b"senders_csv")?
                        .and_then(|v| String::from_utf8(v).ok())
                        .unwrap_or_default();
                    let mut out: Vec<String> = Vec::new();
                    for part in existing.split(',').chain(m.senders_csv.split(',')) {
                        let p = part.trim();
                        if !p.is_empty() && !out.iter().any(|s| s.eq_ignore_ascii_case(p)) {
                            out.push(p.to_string());
                        }
                    }
                    out.join(",").into_bytes()
                };
                // The row's display fields + list position follow the last
                // INBOUND message only. The user's own reply must not
                // re-date or re-title the Inbox row (2026-07-18) — an own
                // write only seeds the fields when the thread is brand new
                // (sent-only thread, nothing to preserve).
                let have_display = ctx.hexists(thread_key.as_bytes(), b"latest_date")?;
                // `search_blob` is the field the full-text index reads;
                // it has to move in lockstep with the three fields it
                // concatenates or search goes stale for this thread.
                if !m.is_own || !have_display {
                    let blob = keys::search_blob(
                        m.subject,
                        &String::from_utf8_lossy(&merged_senders),
                        m.latest_preview,
                    )
                    .into_bytes();
                    let pairs: &[(&[u8], &[u8])] = &[
                        (b"subject", &subj),
                        (b"senders_csv", &merged_senders),
                        (b"latest_date", &date_s),
                        (b"latest_preview", &preview),
                        (b"category", &category),
                        (keys::THREAD_SEARCH_FIELD, &blob),
                    ];
                    ctx.hset(thread_key.as_bytes(), pairs)?;
                } else {
                    // own send: only the participant union changed, but the
                    // blob embeds it, so refresh both.
                    let cur_subject = ctx
                        .hget(thread_key.as_bytes(), b"subject")?
                        .and_then(|v| String::from_utf8(v).ok())
                        .unwrap_or_default();
                    let cur_preview = ctx
                        .hget(thread_key.as_bytes(), b"latest_preview")?
                        .and_then(|v| String::from_utf8(v).ok())
                        .unwrap_or_default();
                    let blob = keys::search_blob(
                        &cur_subject,
                        &String::from_utf8_lossy(&merged_senders),
                        &cur_preview,
                    )
                    .into_bytes();
                    ctx.hset(
                        thread_key.as_bytes(),
                        &[
                            (b"senders_csv" as &[u8], merged_senders.as_slice()),
                            (keys::THREAD_SEARCH_FIELD, blob.as_slice()),
                        ],
                    )?;
                }
                // Atomic counters. The membership row is derived from
                // the thread hash afterwards, so these no longer need
                // to be read back here — but every increment still has
                // to happen.
                ctx.hincrby(thread_key.as_bytes(), b"count", 1)?;
                if m.is_own {
                    ctx.hincrby(thread_key.as_bytes(), b"sent_count", 1)?;
                }
                if m.unread && !m.is_own {
                    ctx.hincrby(thread_key.as_bytes(), b"unread_count", 1)?;
                }

                Ok(())
            })
            .map_err(std::io::Error::other)?;

        // Membership row for the declared table. This is the main
        // ingest path and it does not go through `upsert_thread`, so
        // without this every arriving thread would be absent from
        // every axis the table serves — the same shape of gap the
        // v2.8.2 comment above describes for the folder zsets.
        //
        // Derived from the thread hash rather than from `m`: the row
        // above is the merge of this arrival with whatever was already
        // there, and the membership row has to describe the merged
        // result, not just this message.
        if let Some(row) = self.get_thread(m.thread_id)? {
            self.write_thread_user_if_changed(m.user, &row)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ListThreadsFilter;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    /// What the served axes say — the zsets these tests used to read
    /// are gone, and these are the same questions asked of the rows
    /// the engine now indexes.
    fn bucket_count(s: &KevyMailboxStore, user: &str, bucket: &str) -> usize {
        s.count_thread_ids_by_bucket_via_table(user, bucket)
            .unwrap()
    }
    fn flag_count(s: &KevyMailboxStore, user: &str, flag: &str) -> usize {
        s.count_thread_ids_by_flag_via_table(user, flag).unwrap()
    }
    fn total_count(s: &KevyMailboxStore, user: &str) -> usize {
        s.count_thread_ids_by_activity_via_table(user).unwrap()
    }
    /// What the Inbox axis actually serves. Distinct from
    /// `bucket_count`: Inbox reads the sent-excluding ORDERPATH, so a
    /// thread the user only ever sent has `bucket = inbox` on its row
    /// and is still correctly absent here.
    fn inbox_ids(s: &KevyMailboxStore, user: &str) -> Vec<String> {
        let f = crate::ListThreadsFilter {
            folder: Some("Inbox"),
            ..Default::default()
        };
        s.list_threads_by_activity(user, &f, 0, 1000)
            .unwrap()
            .0
            .into_iter()
            .map(|r| r.thread_id)
            .collect()
    }
    fn in_bucket(s: &KevyMailboxStore, user: &str, bucket: &str, tid: &str) -> bool {
        s.list_thread_ids_by_bucket_via_table(user, bucket, 1000)
            .unwrap()
            .iter()
            .any(|t| t == tid)
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
            // test shorthand: unread=false rows model the user's own sends
            is_own: !unread,
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
        // reachable on the default, bucket and unread axes
        assert_eq!(total_count(&s, "u@x.com"), 1);
        assert_eq!(bucket_count(&s, "u@x.com", "inbox"), 1);
        assert_eq!(flag_count(&s, "u@x.com", "unread"), 1);
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
        // and the axis still holds exactly the one thread
        assert_eq!(total_count(&s, "u@x.com"), 1);
    }

    #[test]
    fn sent_message_bumps_sent_count_not_unread() {
        let s = store();
        s.record_message_arrival(&arr("t1", "u@x.com", "Outgoing", 100, false))
            .unwrap();
        let row = s.get_thread("t1").unwrap().unwrap();
        assert_eq!(row.sent_count, 1);
        assert_eq!(row.unread_count, 0);
        // Without any unread, the unread axis stays empty.
        assert_eq!(flag_count(&s, "u@x.com", "unread"), 0);
    }

    #[test]
    fn inbound_arrival_joins_inbox_folder() {
        let s = store();
        s.record_message_arrival(&arr("t1", "u@x.com", "Hi", 100, true))
            .unwrap();
        assert!(in_bucket(&s, "u@x.com", "inbox", "t1"));
        assert_eq!(bucket_count(&s, "u@x.com", "junk"), 0);
    }

    #[test]
    fn spam_arrival_joins_junk_not_inbox() {
        let s = store();
        let mut a = arr("t1", "u@x.com", "V1AGRA", 100, true);
        a.category = "spam";
        s.record_message_arrival(&a).unwrap();
        assert_eq!(bucket_count(&s, "u@x.com", "inbox"), 0);
        assert!(in_bucket(&s, "u@x.com", "junk", "t1"));
    }

    #[test]
    fn notification_and_promotion_arrivals_join_their_buckets() {
        let s = store();
        let u = "u@x.com";
        let mut n = arr("tn", u, "GitHub notice", 100, true);
        n.category = "notification";
        s.record_message_arrival(&n).unwrap();
        let mut p = arr("tp", u, "50% off", 200, true);
        p.category = "promotion";
        s.record_message_arrival(&p).unwrap();

        // Each lands only in its own bucket, never Inbox.
        assert_eq!(bucket_count(&s, u, "inbox"), 0);
        assert!(in_bucket(&s, u, "notifications", "tn"));
        assert!(in_bucket(&s, u, "promotions", "tp"));
        assert_eq!(bucket_count(&s, u, "notifications"), 1);
        assert_eq!(bucket_count(&s, u, "promotions"), 1);
    }

    #[test]
    fn np_folder_lists_union_of_notifications_and_promotions() {
        let s = store();
        let u = "u@x.com";
        let mut n = arr("tn", u, "notice", 100, true);
        n.category = "notification";
        s.record_message_arrival(&n).unwrap();
        let mut p = arr("tp", u, "promo", 200, true);
        p.category = "promotion";
        s.record_message_arrival(&p).unwrap();
        // A plain inbox thread must NOT appear in the np view.
        s.record_message_arrival(&arr("ti", u, "hi", 150, true))
            .unwrap();

        let f = ListThreadsFilter {
            folder: Some("np"),
            ..Default::default()
        };
        let (rows, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
        assert_eq!(total, 2);
        // newest-first: promo (200) then notice (100).
        assert_eq!(rows[0].thread_id, "tp");
        assert_eq!(rows[1].thread_id, "tn");
    }

    #[test]
    fn sent_only_thread_stays_out_of_inbox() {
        let s = store();
        // Outbound-only write: count == sent_count → Sent axis only.
        s.record_message_arrival(&arr("t1", "u@x.com", "Outgoing", 100, false))
            .unwrap();
        assert!(inbox_ids(&s, "u@x.com").is_empty());
        // A reply arriving later promotes the thread into Inbox.
        s.record_message_arrival(&arr("t1", "u@x.com", "Re: Outgoing", 200, true))
            .unwrap();
        assert_eq!(inbox_ids(&s, "u@x.com"), vec!["t1".to_string()]);
    }

    #[test]
    fn a_sent_only_thread_never_reaches_the_inbox_axis() {
        let s = store();
        let u = "u@x.com";
        // A sent-only arrival must not surface in Inbox — the axis
        // excludes it by construction now rather than by a scrubbing
        // step, so there is no stale state left to heal.
        s.record_message_arrival(&arr("t1", u, "Outgoing", 100, false))
            .unwrap();
        assert!(inbox_ids(&s, u).is_empty());
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

    #[test]
    fn own_reply_does_not_redate_or_reposition_the_row() {
        let s = store();
        let u = "u@x.com";
        // inbound from alice at t=100, then the user replies at t=900
        s.record_message_arrival(&arr("t1", u, "Hello", 100, true))
            .unwrap();
        let reply = MessageArrival {
            thread_id: "t1",
            user: u,
            subject: "Re: Hello",
            senders_csv: u,
            latest_date: 900,
            latest_preview: "my reply",
            category: "inbox",
            unread: false,
            is_own: true,
        };
        s.record_message_arrival(&reply).unwrap();

        let row = s.get_thread("t1").unwrap().unwrap();
        // display fields stay at the inbound message
        assert_eq!(row.latest_date, 100);
        assert_eq!(row.subject, "Hello");
        assert_eq!(row.latest_preview, "preview text");
        assert_eq!(row.count, 2);
        assert_eq!(row.sent_count, 1);
        // the reply's sender still joins the participant union
        assert!(row.senders_csv.contains("alice@x.com"));
        assert!(row.senders_csv.contains(u));
        // list position also stays at inbound time — the membership
        // row's activity is what orders the axes now.
        assert!(in_bucket(&s, u, "inbox", "t1"));
    }

    #[test]
    fn inbound_after_own_reply_advances_the_row() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u, "Hello", 100, true))
            .unwrap();
        let reply = MessageArrival {
            thread_id: "t1",
            user: u,
            subject: "Re: Hello",
            senders_csv: u,
            latest_date: 900,
            latest_preview: "my reply",
            category: "inbox",
            unread: false,
            is_own: true,
        };
        s.record_message_arrival(&reply).unwrap();
        // alice answers at t=1000 — NOW the row advances
        s.record_message_arrival(&arr("t1", u, "Re: Hello", 1000, true))
            .unwrap();
        let row = s.get_thread("t1").unwrap().unwrap();
        assert_eq!(row.latest_date, 1000);
        assert_eq!(total_count(&s, u), 1);
    }
}
