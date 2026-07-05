//! Mailbox-id index for the core-api mailbox routes. The maildir IMAP
//! backend keys mailboxes by (user, name); the contract keys them by an
//! i64 `MailboxId`. This maps a STABLE hashed id back to (user, name) so
//! `get_mailbox_by_id` / `mailbox_status` (which receive only the id) can
//! resolve it. Layout: `mailrs:mbid:<id>` string = "user\nname".

use std::hash::{Hash, Hasher};
use std::io;

use crate::KevyMailboxStore;

/// Deterministic i64 id for a (user, name) mailbox. `DefaultHasher::new()`
/// uses fixed keys, so this is stable across runs and processes.
pub fn mailbox_id(user: &str, name: &str) -> i64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    user.hash(&mut h);
    0u8.hash(&mut h);
    name.hash(&mut h);
    (h.finish() >> 1) as i64
}

impl KevyMailboxStore {
    /// Record `id -> (user, name)` so a bare-id route can resolve it.
    pub fn register_mailbox_id(&self, user: &str, name: &str) -> io::Result<i64> {
        let id = mailbox_id(user, name);
        let val = format!("{user}\n{name}");
        self.store()
            .set(format!("mailrs:mbid:{id}").as_bytes(), val.as_bytes())?;
        Ok(id)
    }

    /// Resolve a mailbox id back to `(user, name)`.
    pub fn lookup_mailbox_id(&self, id: i64) -> io::Result<Option<(String, String)>> {
        let Some(raw) = self.store().get(format!("mailrs:mbid:{id}").as_bytes())? else {
            return Ok(None);
        };
        let s = String::from_utf8_lossy(&raw);
        let mut parts = s.splitn(2, '\n');
        match (parts.next(), parts.next()) {
            (Some(u), Some(n)) => Ok(Some((u.to_string(), n.to_string()))),
            _ => Ok(None),
        }
    }

    /// Drop a mailbox-id mapping (on delete).
    pub fn forget_mailbox_id(&self, id: i64) -> io::Result<()> {
        self.store()
            .del(&[format!("mailrs:mbid:{id}").as_bytes()])?;
        Ok(())
    }
}
