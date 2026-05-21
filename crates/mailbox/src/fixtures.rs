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
