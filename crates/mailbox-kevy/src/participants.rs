//! Who a conversation is with, for one reader: the `senders_csv` on their
//! membership row, which is what the list shows.
//!
//! It is the senders of the messages this reader holds, in date order, the
//! way the SQL core aggregates it. The shared thread hash keeps its own
//! `senders_csv`, the union of every sender any member received; the
//! membership row used to be copied from it, so a member who joined a
//! conversation later listed people who never wrote to them.
//! `thread_user_pairs` no longer emits the field, and [`write_participants`]
//! is the only thing that writes it.
//!
//! Two ways in:
//! - an arrival adds its own sender ([`KevyMailboxStore::add_participants`]),
//!   which does not depend on whether the message row was written before or
//!   after the arrival (the delivery and the sent-copy routes differ);
//! - anything that takes a message out of the reader's conversation or puts
//!   one in without an arrival (expunge, split, merge, a row created without
//!   an arrival) recounts from the reader's own messages
//!   ([`KevyMailboxStore::recount_participants`]).
//!
//! [`write_participants`]: KevyMailboxStore::write_participants

use std::io;

use super::KevyMailboxStore;
use super::keys;

const FIELD: &[u8] = b"senders_csv";

impl KevyMailboxStore {
    /// The senders of the messages `user` holds in `thread_id`, oldest
    /// first, each once.
    pub fn participants_from_messages(&self, user: &str, thread_id: &str) -> io::Result<String> {
        let mids = self.user_thread_message_ids(user, thread_id)?;
        let mut senders = Vec::with_capacity(mids.len());
        for mid in &mids {
            let Some(blob) = self.get_message(mid)? else {
                continue;
            };
            // Messages stored before intake decoded headers carry the
            // encoded words; the list shows the names, as arrivals write them.
            if let Ok(w) = serde_json::from_slice::<Sender>(&blob) {
                senders.push(mailrs_rfc2047::decode(w.sender.as_bytes()).into_owned());
            }
        }
        Ok(mailrs_mailbox::participants::union(
            senders.iter().map(String::as_str),
        ))
    }

    /// Put `user`'s list for `thread_id` back to what their own messages
    /// say. Returns whether the row differs (and, unless `dry_run`, was
    /// written). A row with no messages of theirs is left alone: the count
    /// cannot answer either, and a conversation with nothing in it is
    /// deleted by whoever removed the last message.
    pub fn recount_participants(
        &self,
        user: &str,
        thread_id: &str,
        dry_run: bool,
    ) -> io::Result<bool> {
        if self.user_thread_message_ids(user, thread_id)?.is_empty() {
            return Ok(false);
        }
        let want = self.participants_from_messages(user, thread_id)?;
        match dry_run {
            true => Ok(self
                .stored_participants(user, thread_id)?
                .is_some_and(|h| h != want)),
            false => self.write_participants(user, thread_id, &want),
        }
    }

    /// Add an arriving message's sender to `user`'s list.
    pub(crate) fn add_participants(
        &self,
        user: &str,
        thread_id: &str,
        senders: &str,
    ) -> io::Result<bool> {
        let have = self
            .stored_participants(user, thread_id)?
            .unwrap_or_default();
        let want = mailrs_mailbox::participants::union([have.as_str(), senders]);
        self.write_participants(user, thread_id, &want)
    }

    /// `user`'s stored list, or `None` when they have no row.
    pub fn stored_participants(&self, user: &str, thread_id: &str) -> io::Result<Option<String>> {
        let key = keys::thread_user(user, thread_id);
        let store = self.store();
        if !store.hexists(key.as_bytes(), b"tid")? {
            return Ok(None);
        }
        let have = store
            .hget(key.as_bytes(), FIELD)?
            .and_then(|v| String::from_utf8(v).ok())
            .unwrap_or_default();
        Ok(Some(have))
    }

    /// The one writer of the field. Only on an existing row, so a list
    /// cannot conjure membership, and only when it changes. Returns whether
    /// it wrote.
    fn write_participants(&self, user: &str, thread_id: &str, csv: &str) -> io::Result<bool> {
        let key = keys::thread_user(user, thread_id);
        let wrote = self.store().atomic(|ctx| {
            if !ctx.hexists(key.as_bytes(), b"tid")? {
                return Ok(false);
            }
            let have = ctx.hget(key.as_bytes(), FIELD)?;
            if have.as_deref() == Some(csv.as_bytes()) {
                return Ok(false);
            }
            ctx.hset(key.as_bytes(), &[(FIELD, csv.as_bytes())])?;
            Ok(true)
        })?;
        Ok(wrote)
    }

    /// A row that has no list yet gets one from the reader's messages: the
    /// first write of a membership row, whichever path created it.
    pub(crate) fn seed_participants(&self, user: &str, thread_id: &str) -> io::Result<()> {
        let key = keys::thread_user(user, thread_id);
        if self.store().hexists(key.as_bytes(), FIELD)? {
            return Ok(());
        }
        self.recount_participants(user, thread_id, false)?;
        Ok(())
    }
}

#[derive(serde::Deserialize)]
struct Sender {
    #[serde(default)]
    sender: String,
}
