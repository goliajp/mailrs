//! The store operations over a thread row — reading it, writing it, and
//! keeping each owner's membership row in step with it.
//!
//! Separated from the row's own shape (see the parent module) because
//! these are what the rest of the crate calls, and the shape is what
//! they agree on.

use std::io;

use super::{PER_USER_FLAGS, ThreadRow, thread_user_pairs};
use crate::KevyMailboxStore;
use crate::keys;

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
        let have: std::collections::HashMap<Vec<u8>, Vec<u8>> =
            self.store().hgetall(key.as_bytes())?.into_iter().collect();
        // A flag the row does not carry is not the same as a flag set to
        // zero. `archived` is an equality component of every ORDERPATH
        // prefix, so a row missing it is in none of them — invisible in
        // every list rather than merely un-archived.
        //
        // Emptiness is the wrong test for that. The arrival path writes
        // this user's three counters to the row inside its own atomic
        // block *before* getting here, so on the main ingest path the row
        // already exists and `fresh` is false — which is how rows created
        // by arriving mail came to have no flag columns at all. Ask which
        // ones are absent instead of whether anything is.
        let missing: Vec<&str> = PER_USER_FLAGS
            .iter()
            .copied()
            .filter(|f| !have.contains_key(f.as_bytes()))
            .collect();
        if missing.is_empty() && want.iter().all(|(k, v)| have.get(k) == Some(v)) {
            return Ok(false);
        }
        let refs: Vec<(&[u8], &[u8])> = want
            .iter()
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
            .collect();
        self.store().hset(key.as_bytes(), &refs)?;
        if !missing.is_empty() {
            self.plant_thread_user_defaults(&key, &missing)?;
        }
        Ok(true)
    }

    /// Give a membership row the per-user flags it is missing, all off.
    ///
    /// A conversation you have just received is not starred, archived or
    /// pinned **by you**, whatever the shared hash says about whoever else
    /// has it. Seeding from there is the leak this migration closes, so
    /// the seed is zero.
    ///
    /// The columns have to exist rather than be absent: they are declared,
    /// and `written_fields_cover_declared_columns` holds the write path to
    /// producing every one of them.
    ///
    /// Only the absent ones, so a row that has already been read or
    /// starred does not get that flag reset by a later arrival — and so
    /// this stays convergent when `backfill_thread_user` sweeps a mailbox
    /// twice.
    fn plant_thread_user_defaults(&self, key: &str, flags: &[&str]) -> io::Result<()> {
        let zeros: Vec<(&[u8], &[u8])> = flags
            .iter()
            .map(|f| (f.as_bytes(), b"0".as_slice()))
            .collect();
        self.store().hset(key.as_bytes(), &zeros)?;
        Ok(())
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
                //
                // A row that did not exist gets its per-user flags at
                // zero, in the same closure, so no reader can catch it
                // between the two writes with the columns missing.
                let fresh = !ctx.hexists(tu_key.as_bytes(), b"tid")?;
                ctx.hset(tu_key.as_bytes(), &tu_refs)?;
                if fresh {
                    let zeros: Vec<(&[u8], &[u8])> = PER_USER_FLAGS
                        .iter()
                        .map(|f| (f.as_bytes(), b"0".as_slice()))
                        .collect();
                    ctx.hset(tu_key.as_bytes(), &zeros)?;
                }

                Ok(())
            })
            .map_err(std::io::Error::from)
    }

    /// Read a single thread row back. Returns `None` if the hash is
    /// empty (deleted or never existed).
    pub fn get_thread(&self, thread_id: &str) -> io::Result<Option<ThreadRow>> {
        let key = keys::thread(thread_id);
        let pairs = self.store().hgetall(key.as_bytes())?;
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
        let pairs = self.store().hgetall(key.as_bytes())?;
        Ok(ThreadRow::from_user_pairs(thread_id.to_string(), &pairs))
    }
}
