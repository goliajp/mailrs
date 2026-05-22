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

    // ===== UID, modseq, mailbox-id boundary tests =====

    #[tokio::test]
    async fn get_message_by_uid_returns_none_for_uid_zero() {
        // UID 0 is never allocated (uidnext starts at 1).
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        assert!(s.get_message_by_uid(mb.id, 0).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn get_message_by_uid_returns_none_for_u32_max() {
        // UID near u32::MAX is in-range but never allocated in a fresh store.
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        assert!(s.get_message_by_uid(mb.id, u32::MAX).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn get_message_by_uid_returns_none_for_wrong_mailbox() {
        // A message in mailbox A is invisible via mailbox B's UID space.
        let s = InMemoryMailboxStore::new();
        let a = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let b = s.create_mailbox(ALICE, "Sent").await.unwrap();
        let m = s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        assert!(s.get_message_by_uid(a.id, m.uid).await.unwrap().is_some());
        assert!(s.get_message_by_uid(b.id, m.uid).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn get_message_returns_none_for_negative_id() {
        let s = InMemoryMailboxStore::new();
        assert!(s.get_message(-1).await.unwrap().is_none());
        assert!(s.get_message(i64::MIN).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn find_by_message_id_empty_string_returns_only_empty_id_matches() {
        // An empty Message-ID is technically allowed; if a message was stored
        // with empty message_id, find should locate it. (Contract clarity.)
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let mut input = msg(ALICE, "INBOX", "x", "");
        input.message_id = "";
        s.insert_message(input).await.unwrap();
        // Empty message-id lookup matches messages with empty stored ids.
        let hit = s.find_by_message_id(ALICE, "").await.unwrap();
        assert!(hit.is_some());
    }

    #[tokio::test]
    async fn find_by_message_id_returns_none_for_unknown_id() {
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        s.insert_message(msg(ALICE, "INBOX", "x", "real-id")).await.unwrap();
        assert!(s.find_by_message_id(ALICE, "ghost-id").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn find_by_message_id_distinguishes_brackets_from_unbracketed() {
        // The store stores message_id verbatim. So "<a@b>" and "a@b" are distinct keys.
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        s.insert_message(msg(ALICE, "INBOX", "x", "<bracketed@host>")).await.unwrap();
        s.insert_message(msg(ALICE, "INBOX", "y", "bare@host")).await.unwrap();
        assert!(s.find_by_message_id(ALICE, "<bracketed@host>").await.unwrap().is_some());
        assert!(s.find_by_message_id(ALICE, "bare@host").await.unwrap().is_some());
        // mismatched bracket form misses
        assert!(s.find_by_message_id(ALICE, "bracketed@host").await.unwrap().is_none());
    }

    // ===== Insert error paths =====

    #[tokio::test]
    async fn insert_into_other_user_mailbox_errors() {
        // Inserting "as ALICE" into BOB's mailbox name should fail — no such mailbox for ALICE.
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(BOB, "INBOX").await.unwrap();
        let r = s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn insert_does_not_share_uid_space_across_mailbox_recreate() {
        // After deleting and recreating a mailbox, uidnext resets to 1.
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let _ = s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        s.delete_mailbox(ALICE, "INBOX").await.unwrap();
        let mb2 = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m2 = s.insert_message(msg(ALICE, "INBOX", "y", "i2")).await.unwrap();
        assert_eq!(m2.uid, 1, "recreated mailbox starts uidnext at 1");
        assert_eq!(mb2.uidnext, 1, "fresh mailbox before insert");
    }

    // ===== expunge with mixed states =====

    #[tokio::test]
    async fn expunge_with_intermixed_deleted_and_normal_returns_ascending() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        for i in 1..=6 {
            let id = format!("id-{i}");
            s.insert_message(msg(ALICE, "INBOX", "x", &id)).await.unwrap();
        }
        // mark some-not-all
        s.add_flags(mb.id, 5, FLAG_DELETED).await.unwrap();
        s.add_flags(mb.id, 2, FLAG_DELETED).await.unwrap();
        s.add_flags(mb.id, 4, FLAG_DELETED).await.unwrap();
        let removed = s.expunge(mb.id).await.unwrap();
        assert_eq!(removed, vec![2, 4, 5], "ascending order regardless of marking order");
        // surviving messages still present
        for uid in [1u32, 3, 6] {
            assert!(s.get_message_by_uid(mb.id, uid).await.unwrap().is_some());
        }
    }

    #[tokio::test]
    async fn expunge_only_affects_target_mailbox() {
        let s = InMemoryMailboxStore::new();
        let a = s.create_mailbox(ALICE, "A").await.unwrap();
        let b = s.create_mailbox(ALICE, "B").await.unwrap();
        let ma = s.insert_message(msg(ALICE, "A", "x", "ma")).await.unwrap();
        let mb_msg = s.insert_message(msg(ALICE, "B", "y", "mb")).await.unwrap();
        s.add_flags(a.id, ma.uid, FLAG_DELETED).await.unwrap();
        s.add_flags(b.id, mb_msg.uid, FLAG_DELETED).await.unwrap();
        let removed_a = s.expunge(a.id).await.unwrap();
        assert_eq!(removed_a, vec![ma.uid]);
        // mailbox B still has its message until its own expunge
        assert!(s.get_message_by_uid(b.id, mb_msg.uid).await.unwrap().is_some());
    }

    // ===== CONDSTORE compare-and-swap edges =====

    #[tokio::test]
    async fn store_flags_if_unchanged_with_zero_baseline_always_succeeds_for_unchanged_message() {
        // unchangedsince==current_modseq should still succeed (boundary case).
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m = s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        let r = s.store_flags_if_unchanged(mb.id, m.uid, FlagOp::Set, FLAG_SEEN, m.modseq).await.unwrap();
        assert!(r.is_some(), "modseq == unchangedsince is treated as success");
    }

    #[tokio::test]
    async fn store_flags_if_unchanged_with_huge_unchangedsince_succeeds() {
        // u64::MAX as unchangedsince should not falsely fail.
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m = s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        let r = s
            .store_flags_if_unchanged(mb.id, m.uid, FlagOp::Set, FLAG_SEEN, u64::MAX)
            .await
            .unwrap();
        assert!(r.is_some());
    }

    #[tokio::test]
    async fn store_flags_if_unchanged_for_missing_message_errors() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let r = s
            .store_flags_if_unchanged(mb.id, 999, FlagOp::Add, FLAG_SEEN, 0)
            .await;
        assert!(r.is_err(), "missing message must error, not silently skip");
    }

    #[tokio::test]
    async fn store_flags_if_unchanged_applies_set_correctly() {
        // Set semantics: replace flags entirely.
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m = s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        s.add_flags(mb.id, m.uid, FLAG_SEEN | FLAG_FLAGGED).await.unwrap();
        let pre = s.get_message_by_uid(mb.id, m.uid).await.unwrap().unwrap();
        let _ = s.store_flags_if_unchanged(mb.id, m.uid, FlagOp::Set, FLAG_ANSWERED, pre.modseq).await.unwrap();
        let post = s.get_message_by_uid(mb.id, m.uid).await.unwrap().unwrap();
        assert_eq!(post.flags, FLAG_ANSWERED, "Set replaces entirely");
    }

    // ===== Flag op error paths =====

    #[tokio::test]
    async fn add_flags_missing_message_errors() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        assert!(s.add_flags(mb.id, 99, FLAG_SEEN).await.is_err());
    }

    #[tokio::test]
    async fn set_flags_missing_message_errors() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        assert!(s.set_flags(mb.id, 99, FLAG_SEEN).await.is_err());
    }

    #[tokio::test]
    async fn remove_flags_missing_message_errors() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        assert!(s.remove_flags(mb.id, 99, FLAG_SEEN).await.is_err());
    }

    #[tokio::test]
    async fn add_flags_zero_is_no_op_but_still_bumps_modseq() {
        // OR with 0 doesn't change the flags but still rotates modseq
        // (matches CONDSTORE behavior — STORE bumps even if "no change").
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m = s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        let before = s.get_message_by_uid(mb.id, m.uid).await.unwrap().unwrap();
        let new_modseq = s.add_flags(mb.id, m.uid, 0).await.unwrap();
        let after = s.get_message_by_uid(mb.id, m.uid).await.unwrap().unwrap();
        assert_eq!(before.flags, after.flags);
        assert!(new_modseq > before.modseq, "modseq bumps even on no-op flag change");
    }

    // ===== copy / move semantics =====

    #[tokio::test]
    async fn copy_to_missing_destination_errors() {
        let s = InMemoryMailboxStore::new();
        let src = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m = s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        assert!(s.copy_message(src.id, m.uid, 99_999).await.is_err());
    }

    #[tokio::test]
    async fn move_to_missing_destination_errors() {
        let s = InMemoryMailboxStore::new();
        let src = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m = s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        assert!(s.move_message(src.id, m.uid, 99_999).await.is_err());
        // source message must still exist (rollback semantic)
        assert!(s.get_message_by_uid(src.id, m.uid).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn copy_preserves_message_metadata() {
        // Copying must preserve sender/subject/etc., only uid/modseq/mailbox_id change.
        let s = InMemoryMailboxStore::new();
        let src = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let dst = s.create_mailbox(ALICE, "Archive").await.unwrap();
        let mut input = msg(ALICE, "INBOX", "Original Subject", "orig-id");
        input.sender = "from@source";
        input.recipients = "to@dest";
        input.size = 12345;
        let inserted = s.insert_message(input).await.unwrap();
        let new_uid = s.copy_message(src.id, inserted.uid, dst.id).await.unwrap();
        let copied = s.get_message_by_uid(dst.id, new_uid).await.unwrap().unwrap();
        assert_eq!(copied.subject, "Original Subject");
        assert_eq!(copied.sender, "from@source");
        assert_eq!(copied.recipients, "to@dest");
        assert_eq!(copied.size, 12345);
        assert_eq!(copied.message_id, "orig-id");
        assert_ne!(copied.id, inserted.id, "copy gets a fresh db id");
        assert_eq!(copied.mailbox_id, dst.id);
    }

    #[tokio::test]
    async fn move_to_same_mailbox_succeeds_with_new_uid() {
        // moving within the same mailbox should still produce a new UID and
        // remove the original (per the impl).
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m = s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        let new_uid = s.move_message(mb.id, m.uid, mb.id).await.unwrap();
        assert_ne!(new_uid, m.uid, "self-move allocates a new UID");
        assert!(s.get_message_by_uid(mb.id, m.uid).await.unwrap().is_none());
        assert!(s.get_message_by_uid(mb.id, new_uid).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn copy_bumps_destination_modseq() {
        let s = InMemoryMailboxStore::new();
        let src = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let dst = s.create_mailbox(ALICE, "Archive").await.unwrap();
        let m = s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        let dst_before = s.get_mailbox_by_id(dst.id).await.unwrap().unwrap();
        s.copy_message(src.id, m.uid, dst.id).await.unwrap();
        let dst_after = s.get_mailbox_by_id(dst.id).await.unwrap().unwrap();
        assert!(dst_after.highest_modseq > dst_before.highest_modseq);
    }

    // ===== query_messages corner cases =====

    #[tokio::test]
    async fn query_messages_limit_zero_returns_empty() {
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        for i in 1..=3 {
            let id = format!("i-{i}");
            s.insert_message(msg(ALICE, "INBOX", "x", &id)).await.unwrap();
        }
        let f = QueryFilter {
            user: Some(ALICE),
            limit: 0,
            ..Default::default()
        };
        let out = s.query_messages(f).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn query_messages_position_beyond_total_returns_empty() {
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        let f = QueryFilter {
            user: Some(ALICE),
            position: 100,
            limit: 50,
            ..Default::default()
        };
        let out = s.query_messages(f).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn query_messages_orders_internal_date_descending() {
        // Newest first per sort_unstable_by_key(Reverse(internal_date)).
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        for (i, ts) in [50i64, 200, 100].into_iter().enumerate() {
            let id = format!("i-{i}");
            let mut input = msg(ALICE, "INBOX", "x", &id);
            input.internal_date = ts;
            s.insert_message(input).await.unwrap();
        }
        let f = QueryFilter {
            user: Some(ALICE),
            limit: 50,
            ..Default::default()
        };
        let out = s.query_messages(f).await.unwrap();
        let dates: Vec<i64> = out.iter().map(|m| m.internal_date).collect();
        assert_eq!(dates, vec![200, 100, 50]);
    }

    #[tokio::test]
    async fn query_messages_text_match_in_sender() {
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let mut a = msg(ALICE, "INBOX", "subj", "i1");
        a.sender = "Charlie <charlie@example.com>";
        let mut b = msg(ALICE, "INBOX", "subj", "i2");
        b.sender = "Dave <dave@example.com>";
        s.insert_message(a).await.unwrap();
        s.insert_message(b).await.unwrap();
        let f = QueryFilter {
            user: Some(ALICE),
            text: Some("charlie"),
            limit: 50,
            ..Default::default()
        };
        let out = s.query_messages(f).await.unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].sender.contains("Charlie"));
    }

    #[tokio::test]
    async fn query_messages_text_no_match_returns_empty() {
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        let f = QueryFilter {
            user: Some(ALICE),
            text: Some("does-not-appear"),
            limit: 50,
            ..Default::default()
        };
        let out = s.query_messages(f).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn query_messages_combined_filters_all_must_match() {
        // mailbox + user + has_keyword + not_keyword + text — all must pass.
        let s = InMemoryMailboxStore::new();
        let inbox = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let archive = s.create_mailbox(ALICE, "Archive").await.unwrap();
        // candidate: INBOX, has SEEN, no FLAGGED, subject contains "match"
        let mut want = msg(ALICE, "INBOX", "perfect-match", "want");
        want.flags = FLAG_SEEN;
        let m_want = s.insert_message(want).await.unwrap();
        // miss: in Archive
        let mut miss_mb = msg(ALICE, "Archive", "perfect-match", "miss-mb");
        miss_mb.flags = FLAG_SEEN;
        s.insert_message(miss_mb).await.unwrap();
        // miss: subject doesn't match
        let mut miss_text = msg(ALICE, "INBOX", "wrong subject", "miss-text");
        miss_text.flags = FLAG_SEEN;
        s.insert_message(miss_text).await.unwrap();

        let f = QueryFilter {
            mailbox_id: Some(inbox.id),
            user: Some(ALICE),
            text: Some("perfect"),
            has_keyword: Some(FLAG_SEEN),
            not_keyword: Some(FLAG_FLAGGED),
            limit: 50,
            ..Default::default()
        };
        let out = s.query_messages(f).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, m_want.id);
        // sanity: Archive mailbox still has its message
        assert_eq!(s.mailbox_status(archive.id).await.unwrap().total, 1);
    }

    // ===== Thread invariants =====

    #[tokio::test]
    async fn thread_id_for_message_returns_none_for_unknown_id() {
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        let r = s.thread_id_for_message(ALICE, "unknown-id").await.unwrap();
        assert!(r.is_none());
    }

    #[tokio::test]
    async fn thread_id_for_message_does_not_leak_across_users() {
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        s.create_mailbox(BOB, "INBOX").await.unwrap();
        let mut alice = msg(ALICE, "INBOX", "x", "shared-id");
        alice.thread_id = "alice-thread";
        let mut bob = msg(BOB, "INBOX", "x", "shared-id");
        bob.thread_id = "bob-thread";
        s.insert_message(alice).await.unwrap();
        s.insert_message(bob).await.unwrap();
        let alice_t = s.thread_id_for_message(ALICE, "shared-id").await.unwrap();
        let bob_t = s.thread_id_for_message(BOB, "shared-id").await.unwrap();
        assert_eq!(alice_t.as_deref(), Some("alice-thread"));
        assert_eq!(bob_t.as_deref(), Some("bob-thread"));
    }

    #[tokio::test]
    async fn thread_message_ids_returns_empty_for_unknown_thread() {
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        let ids = s.thread_message_ids(ALICE, "ghost-thread").await.unwrap();
        assert!(ids.is_empty());
    }

    #[tokio::test]
    async fn thread_message_ids_excludes_other_users_same_thread() {
        // Two users with messages sharing the same thread_id string must not bleed.
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        s.create_mailbox(BOB, "INBOX").await.unwrap();
        let mut a = msg(ALICE, "INBOX", "x", "a1");
        a.thread_id = "T";
        let mut b = msg(BOB, "INBOX", "x", "b1");
        b.thread_id = "T";
        let ia = s.insert_message(a).await.unwrap();
        let ib = s.insert_message(b).await.unwrap();
        let alice_ids = s.thread_message_ids(ALICE, "T").await.unwrap();
        let bob_ids = s.thread_message_ids(BOB, "T").await.unwrap();
        assert_eq!(alice_ids, vec![ia.id]);
        assert_eq!(bob_ids, vec![ib.id]);
    }

    #[tokio::test]
    async fn thread_references_excludes_same_timestamp_messages() {
        // thread_references uses strict <, so messages with equal internal_date are excluded.
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let mut a = msg(ALICE, "INBOX", "x", "a");
        a.thread_id = "T";
        a.internal_date = 100;
        let mut b = msg(ALICE, "INBOX", "x", "b");
        b.thread_id = "T";
        b.internal_date = 100; // same as a
        s.insert_message(a).await.unwrap();
        let ib = s.insert_message(b).await.unwrap();
        let refs = s.thread_references(ib.id).await.unwrap();
        // strict less-than: messages at same ts are excluded
        assert!(refs.is_empty(), "strict-< excludes same-timestamp messages");
    }

    #[tokio::test]
    async fn thread_references_returns_empty_when_message_has_no_thread_id() {
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let mut input = msg(ALICE, "INBOX", "x", "i1");
        input.thread_id = ""; // explicitly empty
        let inserted = s.insert_message(input).await.unwrap();
        let refs = s.thread_references(inserted.id).await.unwrap();
        assert!(refs.is_empty(), "no thread id => no references");
    }

    // ===== messages_changed_since edges =====

    #[tokio::test]
    async fn messages_changed_since_zero_returns_all_messages() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        for i in 1..=3 {
            let id = format!("i-{i}");
            s.insert_message(msg(ALICE, "INBOX", "x", &id)).await.unwrap();
        }
        let out = s.messages_changed_since(mb.id, 0).await.unwrap();
        assert_eq!(out.len(), 3);
    }

    #[tokio::test]
    async fn messages_changed_since_high_modseq_returns_empty() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        let out = s.messages_changed_since(mb.id, u64::MAX).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn messages_changed_since_only_includes_target_mailbox() {
        let s = InMemoryMailboxStore::new();
        let a = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let b = s.create_mailbox(ALICE, "Archive").await.unwrap();
        s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        s.insert_message(msg(ALICE, "Archive", "x", "i2")).await.unwrap();
        let out_a = s.messages_changed_since(a.id, 0).await.unwrap();
        let out_b = s.messages_changed_since(b.id, 0).await.unwrap();
        assert_eq!(out_a.len(), 1);
        assert_eq!(out_b.len(), 1);
        assert_ne!(out_a[0].id, out_b[0].id);
    }

    // ===== Rename + lifecycle =====

    #[tokio::test]
    async fn rename_preserves_messages_and_id() {
        // After rename, the mailbox id stays the same, messages remain accessible
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "Drafts").await.unwrap();
        let m = s.insert_message(msg(ALICE, "Drafts", "x", "i1")).await.unwrap();
        s.rename_mailbox(ALICE, "Drafts", "Outbox").await.unwrap();
        // mailbox id unchanged
        let after = s.get_mailbox_by_id(mb.id).await.unwrap().unwrap();
        assert_eq!(after.name, "Outbox");
        // message still findable
        assert!(s.get_message_by_uid(mb.id, m.uid).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn create_mailbox_idempotent_with_empty_name() {
        // Edge: empty name. Implementation accepts it; same-name idempotency
        // applies just like any other name.
        let s = InMemoryMailboxStore::new();
        let a = s.create_mailbox(ALICE, "").await.unwrap();
        let b = s.create_mailbox(ALICE, "").await.unwrap();
        assert_eq!(a.id, b.id);
    }

    // ===== Volume / memory sanity =====

    #[tokio::test]
    async fn many_inserts_remain_accessible() {
        // Sanity: 500 messages stored and accessible (memory budget probe).
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        for i in 0..500 {
            let id = format!("id-{i:04}");
            s.insert_message(msg(ALICE, "INBOX", "x", &id)).await.unwrap();
        }
        let status = s.mailbox_status(mb.id).await.unwrap();
        assert_eq!(status.total, 500);
        // First and last UID accessible
        assert!(s.get_message_by_uid(mb.id, 1).await.unwrap().is_some());
        assert!(s.get_message_by_uid(mb.id, 500).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn concurrent_inserts_allocate_distinct_uids() {
        // Two tokio::join!'d inserts must produce distinct UIDs (RwLock guards
        // the critical section).
        let s = std::sync::Arc::new(InMemoryMailboxStore::new());
        s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let s1 = s.clone();
        let s2 = s.clone();
        let (r1, r2) = tokio::join!(
            async move { s1.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap() },
            async move { s2.insert_message(msg(ALICE, "INBOX", "y", "i2")).await.unwrap() },
        );
        assert_ne!(r1.uid, r2.uid);
        // and they're 1 and 2 in some order
        let mut uids = vec![r1.uid, r2.uid];
        uids.sort();
        assert_eq!(uids, vec![1, 2]);
    }

    #[tokio::test]
    async fn concurrent_flag_ops_serialize() {
        // Two flag operations on the same UID must both apply, modseq strictly increasing.
        let s = std::sync::Arc::new(InMemoryMailboxStore::new());
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let m = s.insert_message(msg(ALICE, "INBOX", "x", "i1")).await.unwrap();
        let s1 = s.clone();
        let s2 = s.clone();
        let (a, b) = tokio::join!(
            async move { s1.add_flags(mb.id, m.uid, FLAG_SEEN).await.unwrap() },
            async move { s2.add_flags(mb.id, m.uid, FLAG_FLAGGED).await.unwrap() },
        );
        assert_ne!(a, b, "two flag ops produce distinct modseqs");
        let final_msg = s.get_message_by_uid(mb.id, m.uid).await.unwrap().unwrap();
        assert_eq!(final_msg.flags & FLAG_SEEN, FLAG_SEEN);
        assert_eq!(final_msg.flags & FLAG_FLAGGED, FLAG_FLAGGED);
    }

    // ===== User-storage edges =====

    #[tokio::test]
    async fn user_storage_bytes_is_zero_for_unknown_user() {
        let s = InMemoryMailboxStore::new();
        assert_eq!(s.user_storage_bytes("ghost@example.com").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn user_storage_bytes_accumulates_across_mailboxes() {
        let s = InMemoryMailboxStore::new();
        s.create_mailbox(ALICE, "A").await.unwrap();
        s.create_mailbox(ALICE, "B").await.unwrap();
        for (m_name, size) in [("A", 100u32), ("B", 250)] {
            let id_str = format!("id-{m_name}");
            let mut input = msg(ALICE, m_name, "x", &id_str);
            input.size = size;
            s.insert_message(input).await.unwrap();
        }
        assert_eq!(s.user_storage_bytes(ALICE).await.unwrap(), 350);
    }

    #[tokio::test]
    async fn user_storage_bytes_decreases_after_expunge() {
        let s = InMemoryMailboxStore::new();
        let mb = s.create_mailbox(ALICE, "INBOX").await.unwrap();
        let mut input = msg(ALICE, "INBOX", "x", "i1");
        input.size = 500;
        let inserted = s.insert_message(input).await.unwrap();
        assert_eq!(s.user_storage_bytes(ALICE).await.unwrap(), 500);
        s.add_flags(mb.id, inserted.uid, FLAG_DELETED).await.unwrap();
        s.expunge(mb.id).await.unwrap();
        assert_eq!(s.user_storage_bytes(ALICE).await.unwrap(), 0);
    }
}
