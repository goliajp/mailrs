//! Conversations nobody holds: the shared thread record is there, but no
//! account has a membership row or a message row in it. A user's delete
//! cannot reach one (only somebody in a thread deletes it), so they are
//! listed and removed by an operator, one at a time.

use std::collections::HashSet;
use std::io;

use super::KevyMailboxStore;
use super::keys;

/// One conversation nobody holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerlessThread {
    pub thread_id: String,
    /// Messages in the shared message index.
    pub messages: u64,
    /// The newest of them, epoch seconds; 0 when it has none.
    pub latest_at: i64,
}

/// What a delete found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerlessDelete {
    /// It was nobody's, and is gone.
    Deleted(OwnerlessThread),
    /// Somebody holds it; nothing was touched.
    Held,
    /// No such conversation.
    Missing,
}

impl KevyMailboxStore {
    /// Every conversation nobody holds, newest first.
    pub fn ownerless_threads(&self) -> io::Result<Vec<OwnerlessThread>> {
        let held = self.held_thread_ids();
        let mut out = Vec::new();
        for tid in self.shared_thread_ids() {
            if held.contains(&tid) || !self.thread_message_owners(&tid)?.is_empty() {
                continue;
            }
            out.push(self.describe(&tid)?);
        }
        out.sort_by(|a, b| {
            b.latest_at
                .cmp(&a.latest_at)
                .then_with(|| a.thread_id.cmp(&b.thread_id))
        });
        Ok(out)
    }

    /// Delete one conversation nobody holds: its shared record, its
    /// message index, and each message's blob and indexed text. Whether
    /// somebody holds it is asked again here, not trusted from a listing.
    pub fn delete_ownerless_thread(&self, thread_id: &str) -> io::Result<OwnerlessDelete> {
        if !self.is_shared_thread(thread_id) {
            return Ok(OwnerlessDelete::Missing);
        }
        if self.held_thread_ids().contains(thread_id)
            || !self.thread_message_owners(thread_id)?.is_empty()
        {
            return Ok(OwnerlessDelete::Held);
        }
        let found = self.describe(thread_id)?;
        let msgs_zset = keys::thread_messages(thread_id);
        let mids: Vec<String> = self
            .store()
            .zrange(msgs_zset.as_bytes(), 0, -1)?
            .into_iter()
            .filter_map(|(m, _)| String::from_utf8(m).ok())
            .collect();
        let mut text_keys = Vec::new();
        for mid in &mids {
            text_keys.push(keys::message_text(mid));
            text_keys.extend(self.attachment_text_keys(mid)?);
        }
        let thread_key = keys::thread(thread_id);
        let deleted = self.store().atomic(|ctx| {
            if !ctx.hexists(thread_key.as_bytes(), keys::THREAD_EXISTS_FIELD)? {
                return Ok(false);
            }
            for mid in &mids {
                ctx.del(&[keys::message_blob(mid).as_bytes()]);
            }
            for key in &text_keys {
                ctx.del(&[key.as_bytes()]);
            }
            ctx.del(&[msgs_zset.as_bytes()]);
            ctx.del(&[thread_key.as_bytes()]);
            Ok(true)
        })?;
        match deleted {
            true => Ok(OwnerlessDelete::Deleted(found)),
            false => Ok(OwnerlessDelete::Missing),
        }
    }

    fn describe(&self, tid: &str) -> io::Result<OwnerlessThread> {
        let scored = self
            .store()
            .zrange(keys::thread_messages(tid).as_bytes(), 0, -1)?;
        let newest_score = scored.iter().map(|(_, s)| *s as i64).max().unwrap_or(0);
        let latest_date = self
            .store()
            .hget(keys::thread(tid).as_bytes(), b"latest_date")?
            .and_then(|v| String::from_utf8(v).ok())
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        Ok(OwnerlessThread {
            thread_id: tid.to_string(),
            messages: scored.len() as u64,
            latest_at: latest_date.max(newest_score),
        })
    }

    fn is_shared_thread(&self, tid: &str) -> bool {
        self.store()
            .hexists(keys::thread(tid).as_bytes(), keys::THREAD_EXISTS_FIELD)
            .unwrap_or(false)
    }

    /// Ids under the shared thread prefix whose record is a thread hash —
    /// the prefix also covers each thread's message index.
    fn shared_thread_ids(&self) -> Vec<String> {
        let mut pattern = keys::THREAD_PREFIX.to_vec();
        pattern.push(b'*');
        let (_, found) = self.store().scan(0, Some(&pattern), usize::MAX);
        found
            .into_iter()
            .filter_map(|k| {
                let tid = String::from_utf8(k.strip_prefix(keys::THREAD_PREFIX)?.to_vec()).ok()?;
                self.is_shared_thread(&tid).then_some(tid)
            })
            .collect()
    }

    /// Thread ids some account has a membership row for. An address has no
    /// `:`, so the first one after the prefix ends it; a thread id may
    /// contain more.
    fn held_thread_ids(&self) -> HashSet<String> {
        let mut pattern = keys::THREAD_USER_PREFIX.to_vec();
        pattern.push(b'*');
        let (_, found) = self.store().scan(0, Some(&pattern), usize::MAX);
        found
            .into_iter()
            .filter_map(|k| {
                let rest = std::str::from_utf8(k.strip_prefix(keys::THREAD_USER_PREFIX)?).ok()?;
                rest.split_once(':').map(|(_, tid)| tid.to_string())
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "ownerless_tests.rs"]
mod tests;
