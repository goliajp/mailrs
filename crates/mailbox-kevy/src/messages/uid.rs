//! IMAP UIDs — allocation, the uid→message-id index, and reads by uid.

use std::io;

use super::KevyMailboxStore;
use super::keys;

impl KevyMailboxStore {
    /// Look up a message by (user, uid) via the per-user uid → message_id
    /// hash. Returns the raw payload bytes (JSON MessageWire) or None
    /// when the uid isn't indexed (or the message was deleted).
    pub fn get_message_by_uid(&self, user: &str, uid: u32) -> io::Result<Option<Vec<u8>>> {
        let idx_key = keys::user_msg_by_uid(user);
        let mid_bytes = self
            .store()
            .hget(idx_key.as_bytes(), uid.to_string().as_bytes())?;
        let Some(mid_bytes) = mid_bytes else {
            return Ok(None);
        };
        let mid = String::from_utf8_lossy(&mid_bytes).to_string();
        // Through the same decision as the thread listing. The uid that
        // found this message is the caller's own, so serving them the shared
        // blob's `blob_ref` — whichever owner wrote last — would open a file
        // in another mailbox, which is what IMAP FETCH and the attachment
        // download would have done for the second owner of a thread.
        self.user_message_view(user, &mid)
    }

    /// Populate the per-user uid → message_id index for a single message.
    /// Called from deliver / migrate paths so per-uid lookups are O(1).
    /// Register a KNOWN (user, uid, message_id) triple — both direction
    /// maps AND raise the allocation counter so future `allocate_uid`
    /// calls never re-issue this uid. This is what migration/backfill
    /// tooling must use: writing only the forward map (the old backfill
    /// behaviour) left `next_uid` at 0, so the first post-migration
    /// delivery allocated uid=1 and overwrote the migrated message's
    /// forward mapping.
    pub fn register_uid(&self, user: &str, uid: u32, message_id: &str) -> io::Result<()> {
        if uid == 0 {
            return Ok(());
        }
        // v2 Stage B.2: rev + forward + counter-max collapsed into one
        // atomic closure. Prior implementation could race the counter
        // read with a concurrent allocate_uid's incr — the pre-fix
        // counter cur could be stale and the conditional set could
        // shrink the counter back below the value allocate_uid already
        // moved past, letting future allocations collide with a uid
        // this backfill just installed.
        let rev_key = keys::user_uid_by_mid(user);
        let idx_key = keys::user_msg_by_uid(user);
        let counter_key = keys::user_next_uid(user);
        self.store()
            .atomic(|ctx| {
                ctx.hset(
                    rev_key.as_bytes(),
                    &[(message_id.as_bytes(), uid.to_string().as_bytes())],
                )?;
                ctx.hset(
                    idx_key.as_bytes(),
                    &[(uid.to_string().as_bytes(), message_id.as_bytes())],
                )?;
                let cur = ctx
                    .get(counter_key.as_bytes())?
                    .and_then(|b| String::from_utf8(b).ok())
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(0);
                if cur < uid as i64 {
                    ctx.set(counter_key.as_bytes(), uid.to_string().as_bytes());
                }
                Ok(())
            })
            .map_err(std::io::Error::from)
    }

    pub fn index_uid(&self, user: &str, uid: u32, message_id: &str) -> io::Result<()> {
        let idx_key = keys::user_msg_by_uid(user);
        self.store().hset(
            idx_key.as_bytes(),
            &[(uid.to_string().as_bytes(), message_id.as_bytes())],
        )?;
        Ok(())
    }

    /// Assign a per-user uid to `message_id` and persist both directions
    /// of the mapping. Idempotent: if the message already has a uid,
    /// the existing value is returned without touching the counter.
    ///
    /// Used by the self-heal path so `/api/mail/messages/{uid}/…`
    /// endpoints (raw source, attachments) can resolve messages that
    /// weren't handed a uid by the monolith migration.
    pub fn allocate_uid(&self, user: &str, message_id: &str) -> io::Result<u32> {
        // v2 Stage B.2 · Phase 2: entire idempotent-check + counter-incr
        // + reverse+forward index write runs inside one shard-write
        // lock. Prior implementation could race between the initial
        // hget miss and the incr — two concurrent allocate_uid calls
        // for the same message_id issued two different uids and left
        // one orphaned in the forward index.
        let rev_key = keys::user_uid_by_mid(user);
        let counter_key = keys::user_next_uid(user);
        let idx_key = keys::user_msg_by_uid(user);
        self.store()
            .atomic(|ctx| {
                if let Some(existing) = ctx.hget(rev_key.as_bytes(), message_id.as_bytes())?
                    && let Ok(s) = std::str::from_utf8(&existing)
                    && let Ok(uid) = s.parse::<u32>()
                {
                    return Ok(uid);
                }
                let uid_i = ctx.incr(counter_key.as_bytes())?;
                let uid = uid_i.clamp(1, u32::MAX as i64) as u32;
                ctx.hset(
                    rev_key.as_bytes(),
                    &[(message_id.as_bytes(), uid.to_string().as_bytes())],
                )?;
                ctx.hset(
                    idx_key.as_bytes(),
                    &[(uid.to_string().as_bytes(), message_id.as_bytes())],
                )?;
                Ok(uid)
            })
            .map_err(std::io::Error::from)
    }
}
