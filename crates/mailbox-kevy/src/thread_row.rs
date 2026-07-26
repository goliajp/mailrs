//! Thread row read/write — the first real method on the kevy backend.
//!
//! Phase 7.5 — uses the 1.15.0 op surface (hset / hgetall + zincrby +
//! zrevrange) to write a thread aggregate and read it back in one
//! round trip per row, replacing the cascading `list_conversations`
//! aggregate (Rock 1) with O(log n) zset lookups.

use std::io;

use super::KevyMailboxStore;
use super::keys;

/// Sent-folder membership predicate — true when the thread's
/// `senders_csv` contains the user's own address (case-insensitive).
/// Used by `upsert_thread` (write path) and by
/// `mailrs-fastcore-backfill-sent` (backfill path).
pub fn senders_csv_contains_user(senders_csv: &str, user: &str) -> bool {
    let user_lc = user.to_lowercase();
    for token in senders_csv.split(',') {
        let t = token.trim().to_lowercase();
        if t.contains(&user_lc) {
            return true;
        }
    }
    false
}

/// Aggregated thread state — one row in `mailrs:thread:<tid>`.
///
/// Stable on-the-wire field names: the kevy hash uses these exact
/// byte strings as field keys so a future debug dump (kevy-cli HGETALL)
/// stays readable.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreadRow {
    pub thread_id: String,
    pub subject: String,
    pub senders_csv: String,
    pub count: i64,
    pub unread_count: i64,
    pub latest_date: i64,
    pub latest_preview: String,
    pub category: String,
    pub importance_level: String,
    pub importance_score: f64,
    pub requires_action: bool,
    pub pinned: bool,
    pub archived: bool,
    pub has_action: bool,
    pub sent_count: i64,
    pub starred: bool,
}

impl ThreadRow {
    /// Every field name a thread row writes. `delete_thread` deletes
    /// exactly this set — kevy has no HCLEAR, so the list has to be
    /// spelled out, and a field missing from it leaves the row
    /// half-alive after a delete. Adding `search_blob` without updating
    /// the delete list did exactly that, and would have left deleted
    /// mail sitting in the search index (caught by
    /// `delete_thread_clears_all_indexes`, 2026-07-19).
    ///
    /// `field_names_match_to_pairs` keeps this honest.
    pub(crate) fn field_names() -> &'static [&'static [u8]] {
        &[
            b"search_blob",
            b"subject",
            b"senders_csv",
            b"count",
            b"unread_count",
            b"latest_date",
            b"latest_preview",
            b"category",
            b"importance_level",
            b"importance_score",
            b"requires_action",
            b"pinned",
            b"archived",
            b"has_action",
            b"sent_count",
            b"starred",
        ]
    }

    fn to_pairs(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        macro_rules! kv {
            ($k:expr, $v:expr) => {
                ($k.as_bytes().to_vec(), $v.into_bytes())
            };
        }
        vec![
            kv!(
                "search_blob",
                keys::search_blob(&self.subject, &self.senders_csv, &self.latest_preview)
            ),
            kv!("subject", self.subject.clone()),
            kv!("senders_csv", self.senders_csv.clone()),
            kv!("count", self.count.to_string()),
            kv!("unread_count", self.unread_count.to_string()),
            kv!("latest_date", self.latest_date.to_string()),
            kv!("latest_preview", self.latest_preview.clone()),
            kv!("category", self.category.clone()),
            kv!("importance_level", self.importance_level.clone()),
            kv!("importance_score", self.importance_score.to_string()),
            kv!("requires_action", (self.requires_action as u8).to_string()),
            kv!("pinned", (self.pinned as u8).to_string()),
            kv!("archived", (self.archived as u8).to_string()),
            kv!("has_action", (self.has_action as u8).to_string()),
            kv!("sent_count", self.sent_count.to_string()),
            kv!("starred", (self.starred as u8).to_string()),
        ]
    }

    pub(crate) fn from_pairs(thread_id: String, pairs: &[(Vec<u8>, Vec<u8>)]) -> Option<Self> {
        if pairs.is_empty() {
            return None;
        }
        let mut subject = String::new();
        let mut senders_csv = String::new();
        let mut count = 0;
        let mut unread_count = 0;
        let mut latest_date = 0;
        let mut latest_preview = String::new();
        let mut category = String::new();
        let mut importance_level = String::new();
        let mut importance_score = 0.0;
        let mut requires_action = false;
        let mut pinned = false;
        let mut archived = false;
        let mut has_action = false;
        let mut sent_count = 0;
        let mut starred = false;
        for (k, v) in pairs {
            let kk = std::str::from_utf8(k).ok()?;
            let vv = std::str::from_utf8(v).ok()?;
            match kk {
                "subject" => subject = vv.into(),
                "senders_csv" => senders_csv = vv.into(),
                "count" => count = vv.parse().unwrap_or(0),
                "unread_count" => unread_count = vv.parse().unwrap_or(0),
                "latest_date" => latest_date = vv.parse().unwrap_or(0),
                "latest_preview" => latest_preview = vv.into(),
                "category" => category = vv.into(),
                "importance_level" => importance_level = vv.into(),
                "importance_score" => importance_score = vv.parse().unwrap_or(0.0),
                "requires_action" => requires_action = vv == "1",
                "pinned" => pinned = vv == "1",
                "archived" => archived = vv == "1",
                "has_action" => has_action = vv == "1",
                "sent_count" => sent_count = vv.parse().unwrap_or(0),
                "starred" => starred = vv == "1",
                _ => {}
            }
        }
        Some(Self {
            thread_id,
            subject,
            senders_csv,
            count,
            unread_count,
            latest_date,
            latest_preview,
            category,
            importance_level,
            importance_score,
            requires_action,
            pinned,
            archived,
            has_action,
            sent_count,
            starred,
        })
    }
}

/// `1` / `0` as the stored bytes for a boolean column. i64-typed in the
/// declaration so `FILTER flag EQ 1` coerces cleanly.
fn flag(v: bool) -> &'static [u8] {
    if v { b"1" } else { b"0" }
}

/// The membership-row fields for one (user, thread) pair.
///
/// Shared by the live write path and the backfill so the two cannot
/// disagree about what a row contains — a drift between "what writes
/// put there" and "what backfill puts there" would be exactly the
/// class of bug this whole migration exists to remove.
/// A bounded tie-breaker derived from the thread id.
///
/// The id is a Message-ID and can exceed kevy's `MAX_STR_COMPONENT`
/// (255 bytes), and a composite orderpath **excludes the whole row**
/// when any component is over that — two threads on prod disappeared
/// from both composites that way. FNV-1a folded into a non-negative
/// i64 is always in range, so the sort stays total without any row
/// being able to fall out of it.
fn tid_ord(tid: &str) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in tid.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (h >> 1) as i64
}

pub(crate) fn thread_user_pairs(user: &str, row: &ThreadRow) -> Vec<(Vec<u8>, Vec<u8>)> {
    let bucket = keys::bucket_of(&row.category);
    // "sent_only" means every message in the thread came from this
    // user — it lives in Sent and nowhere else. Merely having replied
    // does not qualify: a conversation the user took part in is still
    // an inbox thread. Reading it as "has ever sent" dropped 190
    // threads from one account's inbox on prod.
    let sent_only = row.count > 0 && row.sent_count >= row.count;
    let _ = user;
    vec![
        (b"user".to_vec(), user.as_bytes().to_vec()),
        (b"tid".to_vec(), row.thread_id.as_bytes().to_vec()),
        (
            b"ord".to_vec(),
            tid_ord(&row.thread_id).to_string().into_bytes(),
        ),
        (b"bucket".to_vec(), bucket.name().as_bytes().to_vec()),
        (b"category".to_vec(), row.category.as_bytes().to_vec()),
        (
            b"activity".to_vec(),
            row.latest_date.to_string().into_bytes(),
        ),
        (b"sent_only".to_vec(), flag(sent_only).to_vec()),
        (b"starred".to_vec(), flag(row.starred).to_vec()),
        (b"archived".to_vec(), flag(row.archived).to_vec()),
        (b"pinned".to_vec(), flag(row.pinned).to_vec()),
        (b"unread".to_vec(), flag(row.unread_count > 0).to_vec()),
        (b"has_action".to_vec(), flag(row.has_action).to_vec()),
    ]
}

impl KevyMailboxStore {
    /// Write the membership row only when it is absent or differs.
    ///
    /// Returns whether anything was written. The read-compare costs one
    /// HGETALL against a write that would otherwise churn the AOF on
    /// every backfill pass over already-correct data.
    pub(crate) fn write_thread_user_if_changed(
        &self,
        user: &str,
        row: &ThreadRow,
    ) -> io::Result<bool> {
        let key = keys::thread_user(user, &row.thread_id);
        let want = thread_user_pairs(user, row);
        let have: std::collections::HashMap<Vec<u8>, Vec<u8>> = self
            .store()
            .hgetall(key.as_bytes())
            .map_err(std::io::Error::other)?
            .into_iter()
            .collect();
        if want.iter().all(|(k, v)| have.get(k) == Some(v)) {
            return Ok(false);
        }
        let refs: Vec<(&[u8], &[u8])> = want
            .iter()
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
            .collect();
        self.store()
            .hset(key.as_bytes(), &refs)
            .map_err(std::io::Error::other)?;
        Ok(true)
    }

    /// Write the thread aggregate hash + bump it to head of every index
    /// zset the row's flags say it belongs to.
    ///
    /// Replaces the SQL fanout in the cascade: one HSET + up to 7 ZADDs
    /// in a single closure, no PG round trip, no group-by aggregation.
    pub fn upsert_thread(&self, user: &str, row: &ThreadRow) -> io::Result<()> {
        // v2 Stage B.1: 1 hset + 7 conditional zadd/zrem now collapse
        // into a single AtomicCtx closure, holding one shard write
        // lock. Prior implementation held 8 independent locks and
        // could race concurrent list_threads calls mid-fanout.
        let key = keys::thread(&row.thread_id);
        let pairs = row.to_pairs();
        let pair_refs: Vec<(&[u8], &[u8])> = pairs
            .iter()
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
            .collect();
        let activity = keys::user_threads_by_activity(user);
        let cat = keys::user_threads_by_category(user, &row.category);
        let pinned = keys::user_threads_pinned(user);
        let archived = keys::user_threads_archived(user);
        let has_unread = keys::user_threads_has_unread(user);
        let has_action = keys::user_threads_has_action(user);
        let starred = keys::user_threads_starred(user);
        let sent = keys::user_threads_sent(user);
        // v2.4.0 Phase 2 / v2.9 triage — top-level bucket zsets. A thread
        // lives in exactly ONE of {inbox, notifications, promotions,
        // junk}, derived purely from `category` via `bucket_of`. Sent
        // remains an orthogonal axis via `is_sender` below (a thread can
        // be both a bucket AND Sent — showing up in the user's Sent view
        // whenever they replied at least once).
        let bucket = keys::bucket_of(&row.category);
        let bucket_zset = bucket.zset(user);
        let other_buckets: Vec<String> = keys::Bucket::all_zsets(user)
            .into_iter()
            .filter(|k| *k != bucket_zset)
            .collect();
        let is_sender = senders_csv_contains_user(&row.senders_csv, user);
        let score = row.latest_date as f64;
        let member: &[u8] = row.thread_id.as_bytes();
        let tu_key = keys::thread_user(user, &row.thread_id);
        let tu_pairs = thread_user_pairs(user, row);
        let tu_refs: Vec<(&[u8], &[u8])> = tu_pairs
            .iter()
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
            .collect();
        self.store()
            .atomic(|ctx| {
                ctx.hset(key.as_bytes(), &pair_refs)?;

                // Membership row for the declared `threaduser` table.
                // Written alongside the zsets, read by nothing yet — the
                // zsets stay authoritative until the read cutover.
                //
                // It carries the per-user facts the twelve zsets encode
                // in their key names, so the engine can maintain the
                // access paths instead of these hand-written zadd/zrem
                // pairs. `bucket` is stored rather than derived because
                // the engine cannot call `bucket_of`.
                ctx.hset(tu_key.as_bytes(), &tu_refs)?;

                ctx.zadd(activity.as_bytes(), &[(score, member)])?;
                ctx.zadd(cat.as_bytes(), &[(score, member)])?;

                if row.pinned {
                    ctx.zadd(pinned.as_bytes(), &[(score, member)])?;
                } else {
                    ctx.zrem(pinned.as_bytes(), &[member])?;
                }
                if row.archived {
                    ctx.zadd(archived.as_bytes(), &[(score, member)])?;
                } else {
                    ctx.zrem(archived.as_bytes(), &[member])?;
                }
                if row.unread_count > 0 {
                    ctx.zadd(has_unread.as_bytes(), &[(score, member)])?;
                } else {
                    ctx.zrem(has_unread.as_bytes(), &[member])?;
                }
                if row.has_action {
                    ctx.zadd(has_action.as_bytes(), &[(score, member)])?;
                } else {
                    ctx.zrem(has_action.as_bytes(), &[member])?;
                }
                if row.starred {
                    ctx.zadd(starred.as_bytes(), &[(score, member)])?;
                } else {
                    ctx.zrem(starred.as_bytes(), &[member])?;
                }

                // Sent-folder index — populated when the user's own email
                // shows up in the thread's senders_csv (i.e. they sent at
                // least one message). Fastcore trusts senders_csv here
                // rather than the pg-dump-provided `sent_count`, which
                // comes from a monolith SQL aggregate that also fires on
                // inbound-direction events and produces false positives.
                if is_sender {
                    ctx.zadd(sent.as_bytes(), &[(score, member)])?;
                } else {
                    ctx.zrem(sent.as_bytes(), &[member])?;
                }
                // v2.9 triage — bucket membership. The thread joins exactly
                // one of {inbox, notifications, promotions, junk} per
                // `bucket_of(category)` and is removed from the other three,
                // so a category flip (e.g. "mark as promotion") migrates
                // cleanly. A sent-only thread (count == sent_count, no
                // received message) belongs to the Sent axis alone and must
                // not surface in any inbound bucket; Junk is the exception.
                if bucket == keys::Bucket::Junk || row.count > row.sent_count {
                    ctx.zadd(bucket_zset.as_bytes(), &[(score, member)])?;
                    for other in &other_buckets {
                        ctx.zrem(other.as_bytes(), &[member])?;
                    }
                } else {
                    // Sent-only: not in any inbound bucket.
                    for z in keys::Bucket::all_zsets(user) {
                        ctx.zrem(z.as_bytes(), &[member])?;
                    }
                }
                Ok(())
            })
            .map_err(std::io::Error::other)
    }

    /// Read a single thread row back. Returns `None` if the hash is
    /// empty (deleted or never existed).
    pub fn get_thread(&self, thread_id: &str) -> io::Result<Option<ThreadRow>> {
        let key = keys::thread(thread_id);
        let pairs = self
            .store()
            .hgetall(key.as_bytes())
            .map_err(std::io::Error::other)?;
        Ok(ThreadRow::from_pairs(thread_id.to_string(), &pairs))
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
    fn membership_backfill_converges() {
        // `periodic-work-must-converge`: the second pass over data that
        // is already right must write nothing. An unconditional hset
        // would be idempotent and still churn the AOF forever.
        let st = store();
        let row = sample("t-converge");
        assert!(
            st.write_thread_user_if_changed("alice@x.com", &row)
                .unwrap(),
            "first write must create the row"
        );
        assert!(
            !st.write_thread_user_if_changed("alice@x.com", &row)
                .unwrap(),
            "second write over identical data must be a no-op"
        );

        let mut moved = row.clone();
        moved.category = "spam".into();
        assert!(
            st.write_thread_user_if_changed("alice@x.com", &moved)
                .unwrap(),
            "a changed field must write again"
        );
    }

    #[test]
    fn field_names_match_to_pairs() {
        // The delete path deletes `field_names()`; the write path writes
        // `to_pairs()`. Any drift between them leaves a partially
        // deleted row behind, so pin them together.
        let written: std::collections::BTreeSet<Vec<u8>> =
            sample("t").to_pairs().into_iter().map(|(k, _)| k).collect();
        let declared: std::collections::BTreeSet<Vec<u8>> = ThreadRow::field_names()
            .iter()
            .map(|f| f.to_vec())
            .collect();
        assert_eq!(
            written, declared,
            "ThreadRow::field_names() must list exactly what to_pairs() writes"
        );
    }

    fn sample(tid: &str) -> ThreadRow {
        ThreadRow {
            thread_id: tid.into(),
            subject: "Hello".into(),
            senders_csv: "alice@x.com,bob@y.com".into(),
            count: 3,
            unread_count: 1,
            latest_date: 1782846047,
            latest_preview: "OTP is 881576".into(),
            category: "inbox".into(),
            importance_level: "normal".into(),
            importance_score: 0.5,
            requires_action: false,
            pinned: true,
            archived: false,
            has_action: true,
            sent_count: 1,
            starred: false,
        }
    }

    #[test]
    fn upsert_then_get_round_trips() {
        let s = store();
        let row = sample("t1");
        s.upsert_thread("u@x.com", &row).unwrap();
        let back = s.get_thread("t1").unwrap().unwrap();
        assert_eq!(back, row);
    }

    #[test]
    fn get_missing_returns_none() {
        let s = store();
        assert!(s.get_thread("nope").unwrap().is_none());
    }

    #[test]
    fn pinned_archived_flags_toggle_zset_membership() {
        let s = store();
        let mut row = sample("t2");
        row.archived = true;
        row.pinned = false;
        s.upsert_thread("u@x.com", &row).unwrap();
        let archived = keys::user_threads_archived("u@x.com");
        let pinned = keys::user_threads_pinned("u@x.com");
        assert_eq!(s.store().zcard(archived.as_bytes()).unwrap(), 1);
        assert_eq!(s.store().zcard(pinned.as_bytes()).unwrap(), 0);

        // flip both
        row.archived = false;
        row.pinned = true;
        s.upsert_thread("u@x.com", &row).unwrap();
        assert_eq!(s.store().zcard(archived.as_bytes()).unwrap(), 0);
        assert_eq!(s.store().zcard(pinned.as_bytes()).unwrap(), 1);
    }

    #[test]
    fn activity_zset_carries_latest_date_score() {
        let s = store();
        let row = sample("t3");
        s.upsert_thread("u@x.com", &row).unwrap();
        let activity = keys::user_threads_by_activity("u@x.com");
        let score = s.store().zscore(activity.as_bytes(), b"t3").unwrap();
        assert_eq!(score, Some(row.latest_date as f64));
    }
}
