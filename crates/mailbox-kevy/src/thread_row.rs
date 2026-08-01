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
/// `senders_csv` names the user's own address.
///
/// Used by `upsert_thread` (write path), by the backfill, and through
/// `thread_user_pairs` it decides the declared `is_sender` column, which is
/// the Sent axis itself.
///
/// Whole-address comparison, via the one stone that knows how to reduce an
/// RFC 5322 mailbox to a comparison key. This matched by **substring**
/// until 2026-07-30, so any address ending with the user's own put a
/// foreign thread in their Sent folder — `a@b.com` matched `xa@b.com`.
/// A ninth hand-rolled address extractor was the alternative; the stone
/// exists so there is no tenth.
pub fn senders_csv_contains_user(senders_csv: &str, user: &str) -> bool {
    mailrs_rfc5322::list_contains(senders_csv, user)
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

impl ThreadRow {
    /// Build a row from a **membership row**'s fields — this user's copy
    /// of the conversation, rather than the shared aggregate.
    ///
    /// The two hashes name the same facts differently in two places:
    /// `activity` is the membership row's `latest_date`, and the flags
    /// are stored as the declared `1`/`0` columns the indexes read. The
    /// counters are per-user and maintained by `hincrby` on the arrival
    /// path, so they are read straight off this row.
    ///
    /// Returns `None` when the row is absent — the user has no copy —
    /// which is the same answer `user_message_view` gives for a message.
    pub fn from_user_pairs(thread_id: String, pairs: &[(Vec<u8>, Vec<u8>)]) -> Option<Self> {
        if pairs.is_empty() {
            return None;
        }
        let mut row = Self {
            thread_id,
            subject: String::new(),
            senders_csv: String::new(),
            count: 0,
            unread_count: 0,
            latest_date: 0,
            latest_preview: String::new(),
            category: String::new(),
            importance_level: String::new(),
            importance_score: 0.0,
            requires_action: false,
            pinned: false,
            archived: false,
            has_action: false,
            sent_count: 0,
            starred: false,
        };
        for (k, v) in pairs {
            let (Ok(kk), Ok(vv)) = (std::str::from_utf8(k), std::str::from_utf8(v)) else {
                continue;
            };
            match kk {
                "subject" => row.subject = vv.into(),
                "senders_csv" => row.senders_csv = vv.into(),
                "count" => row.count = vv.parse().unwrap_or(0),
                "unread_count" => row.unread_count = vv.parse().unwrap_or(0),
                "sent_count" => row.sent_count = vv.parse().unwrap_or(0),
                // The membership row calls it `activity`: it is the
                // column every ORDERPATH sorts on, so the name says what
                // it is for rather than where it came from.
                "activity" => row.latest_date = vv.parse().unwrap_or(0),
                "latest_preview" => row.latest_preview = vv.into(),
                "category" => row.category = vv.into(),
                "importance_level" => row.importance_level = vv.into(),
                "importance_score" => row.importance_score = vv.parse().unwrap_or(0.0),
                "requires_action" => row.requires_action = vv == "1",
                "pinned" => row.pinned = vv == "1",
                "archived" => row.archived = vv == "1",
                "has_action" => row.has_action = vv == "1",
                "starred" => row.starred = vv == "1",
                _ => {}
            }
        }
        Some(row)
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
    // Distinct from `sent_only`: the Sent folder shows every thread
    // the user has written in, the way Gmail does, while the inbox
    // only excludes threads that are *nothing but* their own messages.
    // A conversation they replied in is in both.
    let is_sender = senders_csv_contains_user(&row.senders_csv, user);
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
        (b"is_sender".to_vec(), flag(is_sender).to_vec()),
        (b"starred".to_vec(), flag(row.starred).to_vec()),
        (b"archived".to_vec(), flag(row.archived).to_vec()),
        (b"pinned".to_vec(), flag(row.pinned).to_vec()),
        (b"unread".to_vec(), flag(row.unread_count > 0).to_vec()),
        (b"has_action".to_vec(), flag(row.has_action).to_vec()),
        // Display payload, so a list page can be served from this row
        // alone instead of joining back to the shared thread hash
        // (RFC 20260730 S1). Undeclared by the TableSpec — nothing
        // indexes or sorts on them — so adding them does not change the
        // spec and does not rebuild 30k rows' indexes at boot.
        //
        // `latest_date` is absent because it is already here as
        // `activity`, and `category` because it is already a column.
        //
        // The three counters are absent for a different reason: they
        // are maintained per user by `hincrby` on the arrival path, and
        // this list is written with `hset`, which would overwrite each
        // increment with the shared row's total.
        (b"subject".to_vec(), row.subject.as_bytes().to_vec()),
        (b"senders_csv".to_vec(), row.senders_csv.as_bytes().to_vec()),
        (
            b"latest_preview".to_vec(),
            row.latest_preview.as_bytes().to_vec(),
        ),
        (
            b"importance_level".to_vec(),
            row.importance_level.as_bytes().to_vec(),
        ),
        (
            b"importance_score".to_vec(),
            row.importance_score.to_string().into_bytes(),
        ),
        (
            b"requires_action".to_vec(),
            flag(row.requires_action).to_vec(),
        ),
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
        let tu_key = keys::thread_user(user, &row.thread_id);
        let tu_pairs = thread_user_pairs(user, row);
        let tu_refs: Vec<(&[u8], &[u8])> = tu_pairs
            .iter()
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
            .collect();
        self.store()
            .atomic(|ctx| {
                ctx.hset(key.as_bytes(), &pair_refs)?;

                // The membership row for the declared `threaduser`
                // table, and now the only thing this writes: every
                // access path the twelve zsets used to encode in their
                // key names is a column here, maintained by the engine.
                // `bucket` is stored rather than derived because the
                // engine cannot call `bucket_of`.
                ctx.hset(tu_key.as_bytes(), &tu_refs)?;

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

    /// One user's copy of a conversation, read from their membership row.
    ///
    /// Distinct from [`Self::get_thread`], which reads the shared
    /// aggregate: on a thread two accounts both received, the shared one
    /// holds whichever owner wrote last. `None` means this user has no
    /// row for it, which is what not having the conversation means.
    pub fn get_thread_for_user(
        &self,
        user: &str,
        thread_id: &str,
    ) -> io::Result<Option<ThreadRow>> {
        let key = keys::thread_user(user, thread_id);
        let pairs = self
            .store()
            .hgetall(key.as_bytes())
            .map_err(std::io::Error::other)?;
        Ok(ThreadRow::from_user_pairs(thread_id.to_string(), &pairs))
    }
}

#[cfg(test)]
mod tests {
    /// `is_sender` is the Sent axis, and this predicate decides it. The
    /// substring form it replaced put a foreign thread in an account's Sent
    /// folder whenever some other address merely ended with the user's own.
    #[test]
    fn sent_membership_needs_the_whole_address() {
        let u = "lihao@golia.jp";
        assert!(senders_csv_contains_user("lihao@golia.jp", u));
        // The form prod actually stores in `senders_csv`.
        assert!(senders_csv_contains_user(
            "GOLIA <lihao@golia.jp>, x@y.com",
            u
        ));
        assert!(senders_csv_contains_user("LiHao@Golia.JP", u));

        assert!(
            !senders_csv_contains_user("notlihao@golia.jp", u),
            "a longer local part is a different mailbox"
        );
        assert!(
            !senders_csv_contains_user("lihao@golia.jp.evil.example", u),
            "a longer domain is a different mailbox"
        );
        assert!(!senders_csv_contains_user("", u));
    }

    use super::*;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        // Reads are served from the declared table, so a test store
        // has to look like a booted one.
        s.ensure_thread_table();
        s
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
    fn pinned_archived_flags_toggle_membership_row() {
        let s = store();
        let field = |tid: &str, name: &str| -> String {
            s.store()
                .hgetall(keys::thread_user("u@x.com", tid).as_bytes())
                .unwrap()
                .into_iter()
                .find(|(f, _)| f == name.as_bytes())
                .map(|(_, v)| String::from_utf8_lossy(&v).into_owned())
                .unwrap_or_default()
        };

        let mut row = sample("t2");
        row.archived = true;
        row.pinned = false;
        s.upsert_thread("u@x.com", &row).unwrap();
        assert_eq!(field("t2", "archived"), "1");
        assert_eq!(field("t2", "pinned"), "0");

        // flip both
        row.archived = false;
        row.pinned = true;
        s.upsert_thread("u@x.com", &row).unwrap();
        assert_eq!(field("t2", "archived"), "0");
        assert_eq!(field("t2", "pinned"), "1");
    }

    #[test]
    fn membership_row_carries_latest_date_as_activity() {
        let s = store();
        let row = sample("t3");
        s.upsert_thread("u@x.com", &row).unwrap();
        let activity = s
            .store()
            .hgetall(keys::thread_user("u@x.com", "t3").as_bytes())
            .unwrap()
            .into_iter()
            .find(|(f, _)| f == b"activity")
            .map(|(_, v)| String::from_utf8_lossy(&v).into_owned())
            .unwrap_or_default();
        assert_eq!(activity, row.latest_date.to_string());
    }
}
