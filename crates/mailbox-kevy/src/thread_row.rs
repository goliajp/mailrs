//! Thread row read/write — the first real method on the kevy backend.
//!
//! Phase 7.5 — uses the 1.15.0 op surface (hset / hgetall + zincrby +
//! zrevrange) to write a thread aggregate and read it back in one
//! round trip per row, replacing the cascading `list_conversations`
//! aggregate (Rock 1) with O(log n) zset lookups.

use std::io;

use super::KevyMailboxStore;
use super::keys;

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
    fn to_pairs(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        macro_rules! kv {
            ($k:expr, $v:expr) => {
                ($k.as_bytes().to_vec(), $v.into_bytes())
            };
        }
        vec![
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

impl KevyMailboxStore {
    /// Write the thread aggregate hash + bump it to head of every index
    /// zset the row's flags say it belongs to.
    ///
    /// Replaces the SQL fanout in the cascade: one HSET + up to 7 ZADDs
    /// in a single closure, no PG round trip, no group-by aggregation.
    pub fn upsert_thread(&self, user: &str, row: &ThreadRow) -> io::Result<()> {
        let key = keys::thread(&row.thread_id);
        let pairs = row.to_pairs();
        let pair_refs: Vec<(&[u8], &[u8])> = pairs
            .iter()
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
            .collect();
        self.store().hset(key.as_bytes(), &pair_refs)?;

        // bump activity zset (always)
        let activity = keys::user_threads_by_activity(user);
        self.store().zadd(
            activity.as_bytes(),
            &[(row.latest_date as f64, row.thread_id.as_bytes())],
        )?;

        // toggle membership in secondary zsets based on row flags
        let cat = keys::user_threads_by_category(user, &row.category);
        self.store().zadd(
            cat.as_bytes(),
            &[(row.latest_date as f64, row.thread_id.as_bytes())],
        )?;

        let pinned = keys::user_threads_pinned(user);
        if row.pinned {
            self.store().zadd(
                pinned.as_bytes(),
                &[(row.latest_date as f64, row.thread_id.as_bytes())],
            )?;
        } else {
            self.store()
                .zrem(pinned.as_bytes(), &[row.thread_id.as_bytes()])?;
        }

        let archived = keys::user_threads_archived(user);
        if row.archived {
            self.store().zadd(
                archived.as_bytes(),
                &[(row.latest_date as f64, row.thread_id.as_bytes())],
            )?;
        } else {
            self.store()
                .zrem(archived.as_bytes(), &[row.thread_id.as_bytes()])?;
        }

        let has_unread = keys::user_threads_has_unread(user);
        if row.unread_count > 0 {
            self.store().zadd(
                has_unread.as_bytes(),
                &[(row.latest_date as f64, row.thread_id.as_bytes())],
            )?;
        } else {
            self.store()
                .zrem(has_unread.as_bytes(), &[row.thread_id.as_bytes()])?;
        }

        let has_action = keys::user_threads_has_action(user);
        if row.has_action {
            self.store().zadd(
                has_action.as_bytes(),
                &[(row.latest_date as f64, row.thread_id.as_bytes())],
            )?;
        } else {
            self.store()
                .zrem(has_action.as_bytes(), &[row.thread_id.as_bytes()])?;
        }

        let starred = keys::user_threads_starred(user);
        if row.starred {
            self.store().zadd(
                starred.as_bytes(),
                &[(row.latest_date as f64, row.thread_id.as_bytes())],
            )?;
        } else {
            self.store()
                .zrem(starred.as_bytes(), &[row.thread_id.as_bytes()])?;
        }

        // Sent-folder index — populated when at least one message in the
        // thread was sent by this user. Enables `folder=Sent` in the UI
        // without a separate mailbox membership check.
        let sent = keys::user_threads_sent(user);
        if row.sent_count > 0 {
            self.store().zadd(
                sent.as_bytes(),
                &[(row.latest_date as f64, row.thread_id.as_bytes())],
            )?;
        } else {
            self.store()
                .zrem(sent.as_bytes(), &[row.thread_id.as_bytes()])?;
        }

        Ok(())
    }

    /// Read a single thread row back. Returns `None` if the hash is
    /// empty (deleted or never existed).
    pub fn get_thread(&self, thread_id: &str) -> io::Result<Option<ThreadRow>> {
        let key = keys::thread(thread_id);
        let pairs = self.store().hgetall(key.as_bytes())?;
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
