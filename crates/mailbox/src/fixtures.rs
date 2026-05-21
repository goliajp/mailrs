//! In-memory [`MailboxStore`](crate::store::MailboxStore) implementation
//! suitable for tests, examples, and downstream-consumer test harnesses.
//!
//! **Intended use is testing.** All state lives in process memory and never
//! persists; do not wire this into a real deployment.
//!
//! Beyond convenience, [`InMemoryMailboxStore`] doubles as a *second*
//! conforming implementation of the trait — proof that [`MailboxStore`] is
//! a genuine abstraction over storage rather than a thin PostgreSQL wrapper.
//! The "outside-in" design test: if a sane in-memory impl needs contortions
//! to satisfy a method, the method probably hides a backend assumption.
//!
//! ## Quick start
//!
//! ```
//! use mailrs_mailbox::fixtures::{InMemoryMailboxStore, EXAMPLE_USER};
//! use mailrs_mailbox::MailboxStore;
//!
//! # async fn run() {
//! let store = InMemoryMailboxStore::new();
//! let inbox = store.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
//! assert_eq!(inbox.name, "INBOX");
//! # }
//! ```

use std::sync::RwLock;

use async_trait::async_trait;

use crate::store::{MailboxStore, StoreError};
use crate::types::{
    FlagOp, InsertMessage, Inserted, Mailbox, MailboxStatus, Message, QueryFilter, FLAG_DELETED,
    FLAG_SEEN,
};

/// Convenience example user used in doc tests and fixture seeds.
pub const EXAMPLE_USER: &str = "alice@example.com";

/// In-memory [`MailboxStore`] backed by `Vec`s under an `RwLock`.
pub struct InMemoryMailboxStore {
    inner: RwLock<Inner>,
}

#[derive(Default)]
struct Inner {
    next_mailbox_id: i64,
    next_message_id: i64,
    mailboxes: Vec<Mailbox>,
    messages: Vec<Message>,
}

impl InMemoryMailboxStore {
    /// Construct an empty store.
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Inner::default()),
        }
    }
}

impl Default for InMemoryMailboxStore {
    fn default() -> Self {
        Self::new()
    }
}

fn err(msg: impl Into<String>) -> StoreError {
    msg.into().into()
}

#[async_trait]
impl MailboxStore for InMemoryMailboxStore {
    // ===== Mailboxes =====

    async fn create_mailbox(&self, user: &str, name: &str) -> Result<Mailbox, StoreError> {
        let mut inner = self.inner.write().unwrap();
        if let Some(existing) = inner
            .mailboxes
            .iter()
            .find(|m| m.user == user && m.name == name)
        {
            return Ok(existing.clone());
        }
        inner.next_mailbox_id += 1;
        let mb = Mailbox {
            id: inner.next_mailbox_id,
            user: user.to_string(),
            name: name.to_string(),
            uidvalidity: 1,
            uidnext: 1,
            highest_modseq: 0,
        };
        inner.mailboxes.push(mb.clone());
        Ok(mb)
    }

    async fn delete_mailbox(&self, user: &str, name: &str) -> Result<bool, StoreError> {
        let mut inner = self.inner.write().unwrap();
        let before = inner.mailboxes.len();
        let mbox_ids: Vec<i64> = inner
            .mailboxes
            .iter()
            .filter(|m| m.user == user && m.name == name)
            .map(|m| m.id)
            .collect();
        inner.mailboxes.retain(|m| !(m.user == user && m.name == name));
        // cascade-delete messages
        inner.messages.retain(|m| !mbox_ids.contains(&m.mailbox_id));
        Ok(inner.mailboxes.len() < before)
    }

    async fn rename_mailbox(&self, user: &str, from: &str, to: &str) -> Result<(), StoreError> {
        let mut inner = self.inner.write().unwrap();
        if let Some(m) = inner
            .mailboxes
            .iter_mut()
            .find(|m| m.user == user && m.name == from)
        {
            m.name = to.to_string();
            Ok(())
        } else {
            Err(err(format!("mailbox '{from}' not found for user '{user}'")))
        }
    }

    async fn list_mailboxes(&self, user: &str) -> Result<Vec<Mailbox>, StoreError> {
        let inner = self.inner.read().unwrap();
        Ok(inner
            .mailboxes
            .iter()
            .filter(|m| m.user == user)
            .cloned()
            .collect())
    }

    async fn get_mailbox(&self, user: &str, name: &str) -> Result<Option<Mailbox>, StoreError> {
        let inner = self.inner.read().unwrap();
        Ok(inner
            .mailboxes
            .iter()
            .find(|m| m.user == user && m.name == name)
            .cloned())
    }

    async fn get_mailbox_by_id(&self, id: i64) -> Result<Option<Mailbox>, StoreError> {
        let inner = self.inner.read().unwrap();
        Ok(inner.mailboxes.iter().find(|m| m.id == id).cloned())
    }

    async fn mailbox_status(&self, mailbox_id: i64) -> Result<MailboxStatus, StoreError> {
        let inner = self.inner.read().unwrap();
        let total = inner
            .messages
            .iter()
            .filter(|m| m.mailbox_id == mailbox_id)
            .count() as u32;
        let unread = inner
            .messages
            .iter()
            .filter(|m| m.mailbox_id == mailbox_id && m.flags & FLAG_SEEN == 0)
            .count() as u32;
        // In-memory impl doesn't track per-session recency.
        Ok(MailboxStatus {
            total,
            unread,
            recent: 0,
        })
    }

    // ===== Messages =====

    async fn insert_message(&self, input: InsertMessage<'_>) -> Result<Inserted, StoreError> {
        let mut inner = self.inner.write().unwrap();
        // Find target mailbox + bump uidnext + highest_modseq atomically
        // (atomic under RwLock write).
        let mbox = inner
            .mailboxes
            .iter_mut()
            .find(|m| m.user == input.user && m.name == input.mailbox_name)
            .ok_or_else(|| err(format!("mailbox '{}' not found", input.mailbox_name)))?;
        let uid = mbox.uidnext;
        let modseq = mbox.highest_modseq + 1;
        mbox.uidnext += 1;
        mbox.highest_modseq = modseq;
        let mailbox_id = mbox.id;

        inner.next_message_id += 1;
        let id = inner.next_message_id;
        inner.messages.push(Message {
            id,
            mailbox_id,
            uid,
            blob_ref: input.blob_ref.to_string(),
            sender: input.sender.to_string(),
            recipients: input.recipients.to_string(),
            subject: input.subject.to_string(),
            date: input.date,
            internal_date: input.internal_date,
            size: input.size,
            flags: input.flags,
            message_id: input.message_id.to_string(),
            in_reply_to: input.in_reply_to.to_string(),
            thread_id: input.thread_id.to_string(),
            modseq,
            user_address: input.user.to_string(),
        });
        Ok(Inserted { id, uid, modseq })
    }

    async fn get_message_by_uid(
        &self,
        mailbox_id: i64,
        uid: u32,
    ) -> Result<Option<Message>, StoreError> {
        let inner = self.inner.read().unwrap();
        Ok(inner
            .messages
            .iter()
            .find(|m| m.mailbox_id == mailbox_id && m.uid == uid)
            .cloned())
    }

    async fn get_message(&self, id: i64) -> Result<Option<Message>, StoreError> {
        let inner = self.inner.read().unwrap();
        Ok(inner.messages.iter().find(|m| m.id == id).cloned())
    }

    async fn find_by_message_id(
        &self,
        user: &str,
        message_id: &str,
    ) -> Result<Option<Message>, StoreError> {
        let inner = self.inner.read().unwrap();
        Ok(inner
            .messages
            .iter()
            .find(|m| m.user_address == user && m.message_id == message_id)
            .cloned())
    }

    async fn copy_message(
        &self,
        src_mailbox: i64,
        uid: u32,
        dst_mailbox: i64,
    ) -> Result<u32, StoreError> {
        let mut inner = self.inner.write().unwrap();
        let src = inner
            .messages
            .iter()
            .find(|m| m.mailbox_id == src_mailbox && m.uid == uid)
            .cloned()
            .ok_or_else(|| err("source message not found"))?;

        let dst = inner
            .mailboxes
            .iter_mut()
            .find(|m| m.id == dst_mailbox)
            .ok_or_else(|| err("destination mailbox not found"))?;
        let new_uid = dst.uidnext;
        dst.uidnext += 1;
        let modseq = dst.highest_modseq + 1;
        dst.highest_modseq = modseq;

        inner.next_message_id += 1;
        let id = inner.next_message_id;
        inner.messages.push(Message {
            id,
            mailbox_id: dst_mailbox,
            uid: new_uid,
            modseq,
            ..src
        });
        Ok(new_uid)
    }

    async fn move_message(
        &self,
        src_mailbox: i64,
        uid: u32,
        dst_mailbox: i64,
    ) -> Result<u32, StoreError> {
        let new_uid = self.copy_message(src_mailbox, uid, dst_mailbox).await?;
        let mut inner = self.inner.write().unwrap();
        inner
            .messages
            .retain(|m| !(m.mailbox_id == src_mailbox && m.uid == uid));
        Ok(new_uid)
    }

    async fn expunge(&self, mailbox_id: i64) -> Result<Vec<u32>, StoreError> {
        let mut inner = self.inner.write().unwrap();
        let mut removed: Vec<u32> = inner
            .messages
            .iter()
            .filter(|m| m.mailbox_id == mailbox_id && m.flags & FLAG_DELETED != 0)
            .map(|m| m.uid)
            .collect();
        removed.sort_unstable();
        inner
            .messages
            .retain(|m| !(m.mailbox_id == mailbox_id && m.flags & FLAG_DELETED != 0));
        Ok(removed)
    }

    // ===== Flags =====

    async fn set_flags(
        &self,
        mailbox_id: i64,
        uid: u32,
        flags: u32,
    ) -> Result<u64, StoreError> {
        apply_flag_op(self, mailbox_id, uid, FlagOp::Set, flags)
    }

    async fn add_flags(
        &self,
        mailbox_id: i64,
        uid: u32,
        flags: u32,
    ) -> Result<u64, StoreError> {
        apply_flag_op(self, mailbox_id, uid, FlagOp::Add, flags)
    }

    async fn remove_flags(
        &self,
        mailbox_id: i64,
        uid: u32,
        flags: u32,
    ) -> Result<u64, StoreError> {
        apply_flag_op(self, mailbox_id, uid, FlagOp::Remove, flags)
    }

    async fn store_flags_if_unchanged(
        &self,
        mailbox_id: i64,
        uid: u32,
        op: FlagOp,
        flags: u32,
        unchangedsince: u64,
    ) -> Result<Option<u64>, StoreError> {
        let mut inner = self.inner.write().unwrap();
        let Some(idx) = inner
            .messages
            .iter()
            .position(|m| m.mailbox_id == mailbox_id && m.uid == uid)
        else {
            return Err(err("message not found"));
        };
        if inner.messages[idx].modseq > unchangedsince {
            return Ok(None);
        }
        let new_flags = match op {
            FlagOp::Set => flags,
            FlagOp::Add => inner.messages[idx].flags | flags,
            FlagOp::Remove => inner.messages[idx].flags & !flags,
        };
        let new_modseq = bump_modseq_inner(&mut inner, mailbox_id);
        inner.messages[idx].flags = new_flags;
        inner.messages[idx].modseq = new_modseq;
        Ok(Some(new_modseq))
    }

    // ===== Threads =====

    async fn thread_id_for_message(
        &self,
        user: &str,
        message_id: &str,
    ) -> Result<Option<String>, StoreError> {
        let inner = self.inner.read().unwrap();
        Ok(inner
            .messages
            .iter()
            .find(|m| m.user_address == user && m.message_id == message_id)
            .map(|m| m.thread_id.clone()))
    }

    async fn thread_message_ids(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Vec<i64>, StoreError> {
        let inner = self.inner.read().unwrap();
        let mut ids: Vec<(i64, i64)> = inner
            .messages
            .iter()
            .filter(|m| m.user_address == user && m.thread_id == thread_id)
            .map(|m| (m.internal_date, m.id))
            .collect();
        ids.sort_unstable_by_key(|(d, _)| *d);
        Ok(ids.into_iter().map(|(_, id)| id).collect())
    }

    async fn thread_references(&self, message_id: i64) -> Result<Vec<i64>, StoreError> {
        let inner = self.inner.read().unwrap();
        let Some(target) = inner.messages.iter().find(|m| m.id == message_id) else {
            return Ok(Vec::new());
        };
        if target.thread_id.is_empty() {
            return Ok(Vec::new());
        }
        let mut older: Vec<&Message> = inner
            .messages
            .iter()
            .filter(|m| m.thread_id == target.thread_id && m.internal_date < target.internal_date)
            .collect();
        // Walk backwards in time — newest-first ordering matches the PG impl.
        older.sort_unstable_by_key(|m| std::cmp::Reverse(m.internal_date));
        Ok(older.into_iter().map(|m| m.id).collect())
    }

    // ===== Changes =====

    async fn messages_changed_since(
        &self,
        mailbox_id: i64,
        modseq: u64,
    ) -> Result<Vec<Message>, StoreError> {
        let inner = self.inner.read().unwrap();
        let mut out: Vec<Message> = inner
            .messages
            .iter()
            .filter(|m| m.mailbox_id == mailbox_id && m.modseq > modseq)
            .cloned()
            .collect();
        out.sort_unstable_by_key(|m| m.modseq);
        Ok(out)
    }

    // ===== Query =====

    async fn query_messages(
        &self,
        filter: QueryFilter<'_>,
    ) -> Result<Vec<Message>, StoreError> {
        let inner = self.inner.read().unwrap();
        let user_filter = filter.user;
        let mut out: Vec<Message> = inner
            .messages
            .iter()
            .filter(|m| {
                if let Some(mb) = filter.mailbox_id
                    && m.mailbox_id != mb
                {
                    return false;
                }
                if let Some(u) = user_filter
                    && m.user_address != u
                {
                    return false;
                }
                if let Some(kw) = filter.has_keyword
                    && m.flags & kw == 0
                {
                    return false;
                }
                if let Some(kw) = filter.not_keyword
                    && m.flags & kw != 0
                {
                    return false;
                }
                if let Some(text) = filter.text {
                    let t = text.to_lowercase();
                    if !m.sender.to_lowercase().contains(&t)
                        && !m.recipients.to_lowercase().contains(&t)
                        && !m.subject.to_lowercase().contains(&t)
                    {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();
        out.sort_unstable_by_key(|m| std::cmp::Reverse(m.internal_date));
        let position = filter.position as usize;
        let limit = filter.limit as usize;
        Ok(out.into_iter().skip(position).take(limit).collect())
    }

    // ===== Quota =====

    async fn user_storage_bytes(&self, user: &str) -> Result<u64, StoreError> {
        let inner = self.inner.read().unwrap();
        Ok(inner
            .messages
            .iter()
            .filter(|m| m.user_address == user)
            .map(|m| u64::from(m.size))
            .sum())
    }
}

fn apply_flag_op(
    store: &InMemoryMailboxStore,
    mailbox_id: i64,
    uid: u32,
    op: FlagOp,
    flags: u32,
) -> Result<u64, StoreError> {
    let mut inner = store.inner.write().unwrap();
    let Some(idx) = inner
        .messages
        .iter()
        .position(|m| m.mailbox_id == mailbox_id && m.uid == uid)
    else {
        return Err(err("message not found"));
    };
    let new_flags = match op {
        FlagOp::Set => flags,
        FlagOp::Add => inner.messages[idx].flags | flags,
        FlagOp::Remove => inner.messages[idx].flags & !flags,
    };
    let new_modseq = bump_modseq_inner(&mut inner, mailbox_id);
    inner.messages[idx].flags = new_flags;
    inner.messages[idx].modseq = new_modseq;
    Ok(new_modseq)
}

fn bump_modseq_inner(inner: &mut Inner, mailbox_id: i64) -> u64 {
    if let Some(mb) = inner.mailboxes.iter_mut().find(|m| m.id == mailbox_id) {
        mb.highest_modseq += 1;
        mb.highest_modseq
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    //! Corner-case tests for [`InMemoryMailboxStore`]. Complement
    //! `tests/trait_contract.rs` (which exercises the public trait surface
    //! shared with [`crate::pg::PgMailboxStore`]); these tests focus on
    //! in-memory-specific invariants, multi-user isolation, modseq
    //! monotonicity, and query-filter combinations that are tedious to
    //! re-test under PG load.
    use super::*;
    use crate::types::{FLAG_ANSWERED, FLAG_FLAGGED, FLAG_RECENT};

    const ALICE: &str = "alice@example.com";
    const BOB: &str = "bob@example.com";

    fn msg<'a>(user: &'a str, mailbox: &'a str, subject: &'a str, mid: &'a str) -> InsertMessage<'a> {
        InsertMessage {
            user,
            mailbox_name: mailbox,
            blob_ref: "blob",
            sender: "from@example.com",
            recipients: "to@example.com",
            subject,
            size: 100,
            date: 1_700_000_000,
            internal_date: 1_700_000_001,
            message_id: mid,
            in_reply_to: "",
            thread_id: mid,
            flags: 0,
        }
    }

    // ===== Mailbox CRUD edge cases =====

    #[tokio::test]
    async fn new_store_lists_no_mailboxes() {
        let s = InMemoryMailboxStore::new();
        assert!(s.list_mailboxes(ALICE).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_mailbox_returns_zero_unread_status() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let status = s.mailbox_status(mb.id).await.unwrap();
        assert_eq!(status.unread, 0);
        assert_eq!(status.total, 0);
    }

    #[tokio::test]
    async fn create_mailbox_isolates_users_with_same_name() {
        let s = InMemoryMailboxStore::new();
        let a = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let b = s.create_mailbox(BOB, "INBOX").await.unwrap();
        assert_ne!(a.id, b.id, "same name across users must produce distinct mailboxes");
    }

    #[tokio::test]
    async fn list_mailboxes_returns_empty_for_unknown_user() {
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let out = s.list_mailboxes("ghost@example.com").await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn get_mailbox_returns_none_for_missing_name() {
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let out = s.get_mailbox(ALICE, "Archive").await.unwrap();
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn get_mailbox_by_id_returns_none_for_negative_id() {
        let s = InMemoryMailboxStore::new();
        assert!(s.get_mailbox_by_id(-1).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn delete_mailbox_does_not_affect_other_users() {
        let s = InMemoryMailboxStore::new();
        let a_inbox = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        s.create_mailbox(BOB, "INBOX").await.unwrap();
        assert!(s.delete_mailbox(ALICE, "INBOX").await.unwrap());
        assert!(s.get_mailbox_by_id(a_inbox.id).await.unwrap().is_none());
        assert!(s.get_mailbox(BOB, "INBOX").await.unwrap().is_some());
    }

    // ===== Insert + modseq monotonicity =====

    #[tokio::test]
    async fn modseq_strictly_increases_across_inserts() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m1 = s.insert_message(msg(ALICE, "INBOX", "a", "id-1")).await.unwrap();
        let m2 = s.insert_message(msg(ALICE, "INBOX", "b", "id-2")).await.unwrap();
        let m3 = s.insert_message(msg(ALICE, "INBOX", "c", "id-3")).await.unwrap();
        assert!(m1.modseq < m2.modseq && m2.modseq < m3.modseq);
        let after = s.get_mailbox_by_id(mb.id).await.unwrap().unwrap();
        assert_eq!(after.highest_modseq, m3.modseq);
    }

    #[tokio::test]
    async fn modseq_bumps_on_flag_change() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m = s.insert_message(msg(ALICE, "INBOX", "x", "id-x")).await.unwrap();
        let after_insert = m.modseq;
        let after_flag = s.add_flags(mb.id, m.uid, FLAG_SEEN).await.unwrap();
        assert!(after_flag > after_insert);
    }

    #[tokio::test]
    async fn modseq_bumps_on_each_flag_op_independently() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m = s.insert_message(msg(ALICE, "INBOX", "x", "id-x")).await.unwrap();
        let a = s.add_flags(mb.id, m.uid, FLAG_SEEN).await.unwrap();
        let b = s.add_flags(mb.id, m.uid, FLAG_FLAGGED).await.unwrap();
        let c = s.remove_flags(mb.id, m.uid, FLAG_SEEN).await.unwrap();
        assert!(a < b && b < c);
    }

    #[tokio::test]
    async fn uid_is_monotonic_per_mailbox_not_global() {
        let s = InMemoryMailboxStore::new();
        let a = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let b = s.create_mailbox(ALICE, "Archive").await.unwrap();
        let a1 = s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        let b1 = s.insert_message(msg(ALICE, "Archive", "y", "i2")).await.unwrap();
        let a2 = s.insert_message(msg(ALICE, "INBOX", "z", "i3")).await.unwrap();
        assert_eq!(a1.uid, 1, "first INBOX uid = 1");
        assert_eq!(a2.uid, 2, "next INBOX uid = 2 (not 3)");
        assert_eq!(b1.uid, 1, "Archive uid is independent");
        let _ = (a.id, b.id);
    }

    #[tokio::test]
    async fn mailbox_status_counts_unread_correctly() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m1 = s.insert_message(msg(ALICE, "INBOX", "a", "i1")).await.unwrap();
        let _m2 = s.insert_message(msg(ALICE, "INBOX", "b", "i2")).await.unwrap();
        s.add_flags(mb.id, m1.uid, FLAG_SEEN).await.unwrap();
        let status = s.mailbox_status(mb.id).await.unwrap();
        assert_eq!(status.total, 2);
        assert_eq!(status.unread, 1, "one seen, one unread");
    }

    // ===== Flag operation semantics =====

    #[tokio::test]
    async fn add_flags_is_bitwise_or() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m = s.insert_message(msg(ALICE, "INBOX", "a", "i1")).await.unwrap();
        s.set_flags(mb.id, m.uid, FLAG_SEEN).await.unwrap();
        s.add_flags(mb.id, m.uid, FLAG_FLAGGED).await.unwrap();
        let fetched = s.get_message(m.id).await.unwrap().unwrap();
        assert_eq!(fetched.flags & FLAG_SEEN, FLAG_SEEN);
        assert_eq!(fetched.flags & FLAG_FLAGGED, FLAG_FLAGGED);
    }

    #[tokio::test]
    async fn remove_flags_clears_only_named_bits() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m = s.insert_message(msg(ALICE, "INBOX", "a", "i1")).await.unwrap();
        s.set_flags(mb.id, m.uid, FLAG_SEEN | FLAG_FLAGGED | FLAG_ANSWERED).await.unwrap();
        s.remove_flags(mb.id, m.uid, FLAG_FLAGGED).await.unwrap();
        let fetched = s.get_message(m.id).await.unwrap().unwrap();
        assert_eq!(fetched.flags & FLAG_FLAGGED, 0);
        assert_eq!(fetched.flags & FLAG_SEEN, FLAG_SEEN);
        assert_eq!(fetched.flags & FLAG_ANSWERED, FLAG_ANSWERED);
    }

    #[tokio::test]
    async fn set_flags_replaces_entire_bitmask() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m = s.insert_message(msg(ALICE, "INBOX", "a", "i1")).await.unwrap();
        s.set_flags(mb.id, m.uid, FLAG_SEEN | FLAG_FLAGGED).await.unwrap();
        s.set_flags(mb.id, m.uid, FLAG_RECENT).await.unwrap();
        let fetched = s.get_message(m.id).await.unwrap().unwrap();
        assert_eq!(fetched.flags, FLAG_RECENT, "set replaces, not merges");
    }

    // ===== Expunge =====

    #[tokio::test]
    async fn expunge_removes_only_flagged_deleted() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m1 = s.insert_message(msg(ALICE, "INBOX", "a", "i1")).await.unwrap();
        let m2 = s.insert_message(msg(ALICE, "INBOX", "b", "i2")).await.unwrap();
        let m3 = s.insert_message(msg(ALICE, "INBOX", "c", "i3")).await.unwrap();
        s.add_flags(mb.id, m2.uid, FLAG_DELETED).await.unwrap();
        let removed = s.expunge(mb.id).await.unwrap();
        assert_eq!(removed, vec![m2.uid]);
        let status = s.mailbox_status(mb.id).await.unwrap();
        assert_eq!(status.total, 2);
        assert!(s.get_message(m1.id).await.unwrap().is_some());
        assert!(s.get_message(m3.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn expunge_returns_empty_when_nothing_deleted() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        s.insert_message(msg(ALICE, "INBOX", "a", "i1")).await.unwrap();
        let removed = s.expunge(mb.id).await.unwrap();
        assert!(removed.is_empty());
    }

    // ===== Multi-user isolation =====

    #[tokio::test]
    async fn find_by_message_id_does_not_leak_across_users() {
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        s.create_mailbox(BOB, "INBOX").await.unwrap();
        s.insert_message(msg(ALICE, "INBOX", "shared", "shared-id")).await.unwrap();
        s.insert_message(msg(BOB, "INBOX", "shared", "shared-id")).await.unwrap();
        let alice_hit = s.find_by_message_id(ALICE, "shared-id").await.unwrap();
        let bob_hit = s.find_by_message_id(BOB, "shared-id").await.unwrap();
        assert!(alice_hit.is_some());
        assert!(bob_hit.is_some());
        assert_ne!(alice_hit.unwrap().id, bob_hit.unwrap().id);
    }

    #[tokio::test]
    async fn user_storage_bytes_sums_only_target_user() {
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        s.create_mailbox(BOB, "INBOX").await.unwrap();
        let mut a_msg = msg(ALICE, "INBOX", "a", "i1");
        a_msg.size = 1000;
        let mut b_msg = msg(BOB, "INBOX", "b", "i2");
        b_msg.size = 5000;
        s.insert_message(a_msg).await.unwrap();
        s.insert_message(b_msg).await.unwrap();
        assert_eq!(s.user_storage_bytes(ALICE).await.unwrap(), 1000);
        assert_eq!(s.user_storage_bytes(BOB).await.unwrap(), 5000);
    }

    // ===== Query filter combinations =====

    #[tokio::test]
    async fn query_messages_filters_by_has_keyword() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m1 = s.insert_message(msg(ALICE, "INBOX", "a", "i1")).await.unwrap();
        let _m2 = s.insert_message(msg(ALICE, "INBOX", "b", "i2")).await.unwrap();
        s.add_flags(mb.id, m1.uid, FLAG_FLAGGED).await.unwrap();
        let filter = QueryFilter {
            mailbox_id: Some(mb.id),
            user: Some(ALICE),
            has_keyword: Some(FLAG_FLAGGED),
            limit: 50,
            ..Default::default()
        };
        let out = s.query_messages(filter).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, m1.id);
    }

    #[tokio::test]
    async fn query_messages_filters_by_not_keyword() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m1 = s.insert_message(msg(ALICE, "INBOX", "a", "i1")).await.unwrap();
        let m2 = s.insert_message(msg(ALICE, "INBOX", "b", "i2")).await.unwrap();
        s.add_flags(mb.id, m1.uid, FLAG_SEEN).await.unwrap();
        let filter = QueryFilter {
            mailbox_id: Some(mb.id),
            user: Some(ALICE),
            not_keyword: Some(FLAG_SEEN),
            limit: 50,
            ..Default::default()
        };
        let out = s.query_messages(filter).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, m2.id);
    }

    #[tokio::test]
    async fn query_messages_filters_by_text_case_insensitive() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        s.insert_message(msg(ALICE, "INBOX", "Important Notice", "i1")).await.unwrap();
        s.insert_message(msg(ALICE, "INBOX", "Daily digest", "i2")).await.unwrap();
        let filter = QueryFilter {
            mailbox_id: Some(mb.id),
            user: Some(ALICE),
            text: Some("important"),
            limit: 50,
            ..Default::default()
        };
        let out = s.query_messages(filter).await.unwrap();
        assert_eq!(out.len(), 1);
    }

    #[tokio::test]
    async fn query_messages_respects_pagination() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        for i in 0..10 {
            let id = format!("id-{i}");
            s.insert_message(msg(ALICE, "INBOX", "x", &id)).await.unwrap();
        }
        let filter = QueryFilter {
            mailbox_id: Some(mb.id),
            user: Some(ALICE),
            position: 3,
            limit: 4,
            ..Default::default()
        };
        let out = s.query_messages(filter).await.unwrap();
        assert_eq!(out.len(), 4);
    }

    #[tokio::test]
    async fn query_messages_returns_empty_for_unknown_user() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        let filter = QueryFilter {
            mailbox_id: Some(mb.id),
            user: Some("ghost@example.com"),
            limit: 50,
            ..Default::default()
        };
        let out = s.query_messages(filter).await.unwrap();
        assert!(out.is_empty());
    }

    // ===== Move + copy semantics =====

    #[tokio::test]
    async fn copy_does_not_share_uid_namespace() {
        let s = InMemoryMailboxStore::new();
        let a = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let b = s.create_mailbox(ALICE, "Archive").await.unwrap();
        let m = s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        let new_uid = s.copy_message(a.id, m.uid, b.id).await.unwrap();
        assert_eq!(new_uid, 1, "destination uidnext starts at 1");
        let original = s.get_message_by_uid(a.id, m.uid).await.unwrap();
        assert!(original.is_some());
    }

    #[tokio::test]
    async fn move_removes_from_source() {
        let s = InMemoryMailboxStore::new();
        let a = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let b = s.create_mailbox(ALICE, "Archive").await.unwrap();
        let m = s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        s.move_message(a.id, m.uid, b.id).await.unwrap();
        let in_inbox = s.get_message_by_uid(a.id, m.uid).await.unwrap();
        assert!(in_inbox.is_none());
    }

    // ===== Thread bookkeeping =====

    #[tokio::test]
    async fn thread_references_returns_empty_for_unknown_message() {
        let s = InMemoryMailboxStore::new();
        let out = s.thread_references(999_999).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn messages_changed_since_returns_only_newer() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m1 = s.insert_message(msg(ALICE, "INBOX", "a", "i1")).await.unwrap();
        let m2 = s.insert_message(msg(ALICE, "INBOX", "b", "i2")).await.unwrap();
        let changed = s.messages_changed_since(mb.id, m1.modseq).await.unwrap();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].id, m2.id);
    }

    // ===== Default trait + ergonomics =====

    #[tokio::test]
    async fn default_is_equivalent_to_new() {
        let a = InMemoryMailboxStore::new();
        let b = InMemoryMailboxStore::default();
        a.create_mailbox(ALICE, "INBOX").await.unwrap();
        b.create_mailbox(ALICE, "INBOX").await.unwrap();
        assert_eq!(
            a.list_mailboxes(ALICE).await.unwrap().len(),
            b.list_mailboxes(ALICE).await.unwrap().len()
        );
    }
}
