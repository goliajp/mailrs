//! Giving stored messages a corrected date.
//!
//! A message's date sits in three places: its shared blob (`internal_date`
//! and `date`), the score of the conversation's message zset, and the score
//! of each holder's own message zset, which their row's `activity` is
//! recounted from. Each holder's row then shows the newest inbound date of
//! their own messages, and the shared hash's `latest_date` the newest any
//! holder's row shows. [`KevyMailboxStore::redate_messages`] is the one
//! writer of all of them; nothing per reader (flags, uid, modseq, the file,
//! membership) is touched.

use std::collections::{BTreeMap, BTreeSet};
use std::io;

use super::KevyMailboxStore;
use super::keys;

/// What one holder's row shows, before and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowRedate {
    pub user: String,
    /// The stored `activity`.
    pub from: i64,
    /// What the reader's messages say with the new dates.
    pub to: i64,
    /// What the reader's messages say with the dates as stored. When it
    /// differs from `from`, the row was off before this change.
    pub recount_before: i64,
}

/// What [`KevyMailboxStore::redate_messages`] found and, unless a dry run,
/// wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Redate {
    pub rows: Vec<RowRedate>,
    pub shared_from: i64,
    pub shared_to: i64,
}

/// Where a row sits in one reader's lists: the prefix of the declared
/// `by_user_bucket_sent` path, and the sort key under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListSlot {
    pub folder: String,
    pub activity: i64,
    pub ord: i64,
}

/// What the holders of one conversation own there, which a redate must not
/// change: their rows (every field but `activity`), their message rows
/// (flags, uid, modseq, file, thread) and who holds what.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Owned {
    pub rows: u64,
    pub messages: u64,
    /// Rows the lists show as unread (the declared `unread` column).
    pub unread_rows: u64,
    /// Message rows without the seen flag.
    pub unseen_messages: u64,
    pub digest: u64,
}

impl Owned {
    pub fn add(&mut self, o: Owned) {
        self.rows += o.rows;
        self.messages += o.messages;
        self.unread_rows += o.unread_rows;
        self.unseen_messages += o.unseen_messages;
        self.digest ^= o.digest;
    }
}

impl KevyMailboxStore {
    /// See [`Owned`].
    pub fn owned_in(&self, thread_id: &str) -> io::Result<Owned> {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut owners = self.thread_message_owners(thread_id)?;
        owners.sort();
        let mut h = DefaultHasher::new();
        let mut out = Owned::default();
        for (user, mid) in &owners {
            let mut facts = self
                .store()
                .hgetall(keys::user_message(user, mid).as_bytes())?;
            facts.sort();
            let flags: u32 = facts
                .iter()
                .find(|(k, _)| k.as_slice() == b"flags")
                .and_then(|(_, v)| std::str::from_utf8(v).ok()?.parse().ok())
                .unwrap_or(0);
            if flags & mailrs_mailbox::types::FLAG_SEEN == 0 {
                out.unseen_messages += 1;
            }
            (user, mid, facts).hash(&mut h);
            out.messages += 1;
        }
        let users: BTreeSet<&str> = owners.iter().map(|(u, _)| u.as_str()).collect();
        for user in users {
            let Some(row) = self.get_thread_for_user(user, thread_id)? else {
                continue;
            };
            let mut fields = self
                .store()
                .hgetall(keys::thread_user(user, thread_id).as_bytes())?;
            fields.retain(|(k, _)| k.as_slice() != b"activity");
            fields.sort();
            if fields
                .iter()
                .any(|(k, v)| k.as_slice() == b"unread" && v.as_slice() == b"1")
            {
                out.unread_rows += 1;
            }
            (user, fields, row.unread_count, row.count).hash(&mut h);
            out.rows += 1;
        }
        out.digest = h.finish();
        Ok(out)
    }

    /// `user`'s row of `thread_id` as the lists order it, or `None` when
    /// they have no row.
    pub fn list_slot(&self, user: &str, thread_id: &str) -> io::Result<Option<ListSlot>> {
        let pairs = self
            .store()
            .hgetall(keys::thread_user(user, thread_id).as_bytes())?;
        let field = |name: &[u8]| {
            pairs
                .iter()
                .find(|(k, _)| k.as_slice() == name)
                .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
        };
        if field(b"tid").is_none() {
            return Ok(None);
        }
        let folder = ["bucket", "sent_only", "archived", "quarantined", "trashed"]
            .iter()
            .map(|f| field(f.as_bytes()).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("|");
        let number = |name: &[u8]| field(name).and_then(|v| v.parse().ok()).unwrap_or(0);
        Ok(Some(ListSlot {
            folder,
            activity: number(b"activity"),
            ord: number(b"ord"),
        }))
    }

    /// Give the messages in `dates` (message id → date) of `thread_id` their
    /// new date, then recount every holder's row and the shared
    /// `latest_date`. Writes only what differs; with `dry_run`, nothing.
    pub fn redate_messages(
        &self,
        thread_id: &str,
        dates: &BTreeMap<String, i64>,
        dry_run: bool,
    ) -> io::Result<Redate> {
        let owners = self.thread_message_owners(thread_id)?;
        let users: BTreeSet<&str> = owners.iter().map(|(u, _)| u.as_str()).collect();
        let mut rows = Vec::new();
        for user in users {
            let Some((_, from)) = self.stored_standing(user, thread_id)? else {
                continue;
            };
            let mut held = self.held_dates(user, thread_id)?;
            let recount_before = Self::newest_of(&held).map_or(from, |h| h.date);
            for h in &mut held {
                if let Some(d) = dates.get(&h.mid) {
                    h.date = *d;
                }
            }
            let to = Self::newest_of(&held).map_or(from, |h| h.date);
            rows.push(RowRedate {
                user: user.to_string(),
                from,
                to,
                recount_before,
            });
        }
        let thread_key = keys::thread(thread_id);
        let shared_from = self
            .store()
            .hget(thread_key.as_bytes(), b"latest_date")?
            .and_then(|v| String::from_utf8(v).ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let shared_to = rows.iter().map(|r| r.to).max().unwrap_or(shared_from);
        let out = Redate {
            rows,
            shared_from,
            shared_to,
        };
        if dry_run {
            return Ok(out);
        }
        for (mid, date) in dates {
            let holders: Vec<&str> = owners
                .iter()
                .filter(|(_, m)| m == mid)
                .map(|(u, _)| u.as_str())
                .collect();
            self.write_message_date(thread_id, mid, *date, &holders)?;
        }
        for r in out.rows.iter().filter(|r| r.from != r.to) {
            self.write_standing(&r.user, thread_id, None, Some(r.to), None)?;
        }
        if shared_to != shared_from && shared_to > 0 {
            self.store().hset(
                thread_key.as_bytes(),
                &[(b"latest_date".as_slice(), shared_to.to_string().as_bytes())],
            )?;
        }
        Ok(out)
    }

    fn write_message_date(
        &self,
        thread_id: &str,
        mid: &str,
        date: i64,
        holders: &[&str],
    ) -> io::Result<()> {
        let blob_key = keys::message_blob(mid);
        let zset = keys::thread_messages(thread_id);
        let user_zsets: Vec<String> = holders
            .iter()
            .map(|u| keys::thread_user_messages(u, thread_id))
            .collect();
        self.store()
            .atomic(|ctx| {
                if let Some(blob) = ctx.get(blob_key.as_bytes())?
                    && let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(&blob)
                    && let Some(obj) = v.as_object_mut()
                {
                    let stale = ["internal_date", "date"]
                        .iter()
                        .any(|f| obj.get(*f).and_then(|d| d.as_i64()) != Some(date));
                    if stale {
                        obj.insert("internal_date".into(), date.into());
                        obj.insert("date".into(), date.into());
                        ctx.set(blob_key.as_bytes(), v.to_string().as_bytes());
                    }
                }
                ctx.zadd(zset.as_bytes(), &[(date as f64, mid.as_bytes())])?;
                for key in &user_zsets {
                    ctx.zadd(key.as_bytes(), &[(date as f64, mid.as_bytes())])?;
                }
                Ok(())
            })
            .map_err(io::Error::from)
    }
}
