//! `impl MailboxStore for PgMailboxStore` — the PG bridge.
//!
//! Each trait method delegates to the corresponding inherent method on
//! `PgMailboxStore` (defined in the sibling `*_ops.rs` files), converting
//! `sqlx::Error` to the trait's opaque [`StoreError`] and mapping the legacy
//! [`MessageMeta`] shape to the slim [`Message`] type.
//!
//! Where the inherent method's signature doesn't match the trait method
//! cleanly (e.g. `mailbox_status` returns `(u32, u32)`, trait wants a
//! struct), this file does the adapter work — never the caller.

use async_trait::async_trait;
use sqlx::Row;

use crate::pg::PgMailboxStore;
use crate::store::{MailboxStore, StoreError};
use crate::types::{
    FlagOp, InsertMessage, Inserted, Mailbox, MailboxStatus, Message, MessageMeta, QueryFilter,
};

/// Look up the owning user via the source mailbox and the destination name
/// via the destination mailbox id — bridges the trait's (src_id, dst_id)
/// shape to the inherent (user, src_id, dst_name) shape.
async fn resolve_mailbox_user_and_dst(
    store: &PgMailboxStore,
    src_mailbox: i64,
    dst_mailbox: i64,
) -> Result<(String, String), StoreError> {
    let src: (String,) = sqlx::query_as("SELECT user_address FROM mailboxes WHERE id = $1")
        .bind(src_mailbox)
        .fetch_one(store.pool())
        .await?;
    let dst: (String,) = sqlx::query_as("SELECT name FROM mailboxes WHERE id = $1")
        .bind(dst_mailbox)
        .fetch_one(store.pool())
        .await?;
    Ok((src.0, dst.0))
}

/// Drop the 5 mailrs-specific fields when converting to the slim trait
/// type. Used by every trait method that reads messages from PG.
fn meta_to_message(m: MessageMeta) -> Message {
    Message {
        id: m.id,
        mailbox_id: m.mailbox_id,
        uid: m.uid,
        blob_ref: m.maildir_id,
        sender: m.sender,
        recipients: m.recipients,
        subject: m.subject,
        date: m.date,
        internal_date: m.internal_date,
        size: m.size,
        flags: m.flags,
        message_id: m.message_id,
        in_reply_to: m.in_reply_to,
        thread_id: m.thread_id,
        modseq: m.modseq,
        user_address: m.user_address,
    }
}

#[async_trait]
impl MailboxStore for PgMailboxStore {
    async fn create_mailbox(&self, user: &str, name: &str) -> Result<Mailbox, StoreError> {
        Self::create_mailbox(self, user, name)
            .await
            .map_err(Into::into)
    }

    async fn delete_mailbox(&self, user: &str, name: &str) -> Result<bool, StoreError> {
        Self::delete_mailbox(self, user, name)
            .await
            .map_err(Into::into)
    }

    async fn rename_mailbox(&self, user: &str, from: &str, to: &str) -> Result<(), StoreError> {
        Self::rename_mailbox(self, user, from, to).await?;
        Ok(())
    }

    async fn list_mailboxes(&self, user: &str) -> Result<Vec<Mailbox>, StoreError> {
        Self::list_mailboxes(self, user).await.map_err(Into::into)
    }

    async fn get_mailbox(&self, user: &str, name: &str) -> Result<Option<Mailbox>, StoreError> {
        Self::get_mailbox(self, user, name)
            .await
            .map_err(Into::into)
    }

    async fn get_mailbox_by_id(&self, id: i64) -> Result<Option<Mailbox>, StoreError> {
        Self::get_mailbox_by_id(self, id).await.map_err(Into::into)
    }

    async fn mailbox_status(&self, mailbox_id: i64) -> Result<MailboxStatus, StoreError> {
        let (total, unread) = Self::mailbox_status(self, mailbox_id).await?;
        // PG impl doesn't track \Recent per session; surface 0.
        Ok(MailboxStatus {
            total,
            unread,
            recent: 0,
        })
    }

    async fn insert_message(&self, input: InsertMessage<'_>) -> Result<Inserted, StoreError> {
        let uid = Self::index_message(
            self,
            input.user,
            input.mailbox_name,
            input.blob_ref,
            input.sender,
            input.recipients,
            input.subject,
            input.size,
            input.internal_date,
            input.message_id,
            input.in_reply_to,
            input.thread_id,
        )
        .await?;

        // Fetch the resulting id + modseq + mailbox_id for the trait's
        // Inserted return. mailbox_id is needed for the flags path below.
        let row: (i64, i64, i64) = sqlx::query_as(
            "SELECT m.id, m.modseq, mb.id FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND mb.name = $2 AND m.uid = $3",
        )
        .bind(input.user)
        .bind(input.mailbox_name)
        .bind(uid as i32)
        .fetch_one(&self.pool)
        .await?;

        let mut inserted = Inserted {
            id: row.0,
            uid,
            modseq: row.1 as u64,
        };

        // If the caller passed non-zero initial flags (IMAP APPEND / a
        // sync delivering a \Seen message), apply them now. index_message
        // inserts with flags=0. set_flags keys on (mailbox_id, uid) — pass
        // the MAILBOX id (row.2), NOT the message id (row.0); the latter
        // made bump_modseq's `UPDATE mailboxes WHERE id=<msg_id>` match no
        // row → "no rows returned" for every flagged insert.
        if input.flags != 0 {
            let modseq = Self::set_flags(self, row.2, uid, input.flags).await?;
            inserted.modseq = modseq;
        }
        Ok(inserted)
    }

    async fn get_message_by_uid(
        &self,
        mailbox_id: i64,
        uid: u32,
    ) -> Result<Option<Message>, StoreError> {
        // The inherent `get_message(mailbox_id, uid)` returns MessageMeta; map it.
        let meta = Self::get_message(self, mailbox_id, uid).await?;
        Ok(meta.map(meta_to_message))
    }

    async fn get_message(&self, id: i64) -> Result<Option<Message>, StoreError> {
        // No user filter — the trait contract trusts the caller has authorized
        // the lookup (matches JMAP's accountId-scoped id semantics).
        let row = sqlx::query(
            "SELECT m.id, m.mailbox_id, m.uid, m.maildir_id, m.sender, m.recipients,
                    m.subject, m.date_epoch, m.size, m.flags, m.internal_date,
                    m.message_id, m.in_reply_to, m.thread_id, m.modseq, mb.user_address
             FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE m.id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| Message {
            id: r.get::<i64, _>(0),
            mailbox_id: r.get::<i64, _>(1),
            uid: r.get::<i32, _>(2) as u32,
            blob_ref: r.get::<String, _>(3),
            sender: r.get::<String, _>(4),
            recipients: r.get::<String, _>(5),
            subject: r.get::<String, _>(6),
            date: r.get::<i64, _>(7),
            size: r.get::<i32, _>(8) as u32,
            flags: r.get::<i32, _>(9) as u32,
            internal_date: r.get::<i64, _>(10),
            message_id: r.get::<String, _>(11),
            in_reply_to: r.get::<String, _>(12),
            thread_id: r.get::<String, _>(13),
            modseq: r.get::<i64, _>(14) as u64,
            user_address: r.get::<String, _>(15),
        }))
    }

    async fn find_by_message_id(
        &self,
        user: &str,
        message_id: &str,
    ) -> Result<Option<Message>, StoreError> {
        let meta = Self::find_message_by_message_id(self, user, message_id).await?;
        Ok(meta.map(meta_to_message))
    }

    async fn copy_message(
        &self,
        src_mailbox: i64,
        uid: u32,
        dst_mailbox: i64,
    ) -> Result<u32, StoreError> {
        let (user, dst_name) = resolve_mailbox_user_and_dst(self, src_mailbox, dst_mailbox).await?;
        Self::copy_message(self, &user, src_mailbox, uid, &dst_name)
            .await?
            .ok_or_else(|| "copy_message: source message not found".into())
    }

    async fn move_message(
        &self,
        src_mailbox: i64,
        uid: u32,
        dst_mailbox: i64,
    ) -> Result<u32, StoreError> {
        let (user, dst_name) = resolve_mailbox_user_and_dst(self, src_mailbox, dst_mailbox).await?;
        Self::move_message(self, &user, src_mailbox, uid, &dst_name)
            .await?
            .ok_or_else(|| "move_message: source message not found".into())
    }

    async fn expunge(&self, mailbox_id: i64) -> Result<Vec<u32>, StoreError> {
        Self::expunge(self, mailbox_id).await.map_err(Into::into)
    }

    async fn set_flags(&self, mailbox_id: i64, uid: u32, flags: u32) -> Result<u64, StoreError> {
        Self::update_flags(self, mailbox_id, uid, flags)
            .await
            .map_err(Into::into)
    }

    async fn add_flags(&self, mailbox_id: i64, uid: u32, flags: u32) -> Result<u64, StoreError> {
        Self::add_flags(self, mailbox_id, uid, flags)
            .await
            .map_err(Into::into)
    }

    async fn remove_flags(&self, mailbox_id: i64, uid: u32, flags: u32) -> Result<u64, StoreError> {
        Self::remove_flags(self, mailbox_id, uid, flags)
            .await
            .map_err(Into::into)
    }

    async fn store_flags_if_unchanged(
        &self,
        mailbox_id: i64,
        uid: u32,
        op: FlagOp,
        flags: u32,
        unchangedsince: u64,
    ) -> Result<Option<u64>, StoreError> {
        // FlagOp is a type alias for FlagAction, so pass through.
        Self::update_flags_if_unchanged(self, mailbox_id, uid, flags, op, unchangedsince)
            .await
            .map_err(Into::into)
    }

    async fn thread_id_for_message(
        &self,
        user: &str,
        message_id: &str,
    ) -> Result<Option<String>, StoreError> {
        Self::find_thread_id_by_message_id(self, user, message_id)
            .await
            .map_err(Into::into)
    }

    async fn thread_message_ids(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Vec<i64>, StoreError> {
        // The inherent `get_thread_message_ids` returns Vec<String> (RFC 5322
        // Message-ID headers). The trait wants Vec<i64> (db primary keys —
        // JMAP-shape emailIds). Different semantic; write a fresh query.
        let rows: Vec<(i64,)> = sqlx::query_as(
            "SELECT m.id FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE m.thread_id = $1 AND mb.user_address = $2
             ORDER BY m.internal_date ASC",
        )
        .bind(thread_id)
        .bind(user)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn thread_references(&self, message_id: i64) -> Result<Vec<i64>, StoreError> {
        // The inherent `get_thread_references(user, message_id_header)` returns
        // RFC 5322 Message-ID headers. Trait wants db ids walking the same
        // thread. Fresh query: same thread_id, internal_date < this message.
        let rows: Vec<(i64,)> = sqlx::query_as(
            "SELECT m2.id FROM messages m1
             JOIN messages m2 ON m2.thread_id = m1.thread_id
             WHERE m1.id = $1
               AND m1.thread_id <> ''
               AND m2.internal_date < m1.internal_date
             ORDER BY m2.internal_date DESC",
        )
        .bind(message_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn messages_changed_since(
        &self,
        mailbox_id: i64,
        modseq: u64,
    ) -> Result<Vec<Message>, StoreError> {
        let metas = Self::list_messages_changed_since(self, mailbox_id, modseq).await?;
        Ok(metas.into_iter().map(meta_to_message).collect())
    }

    async fn query_messages(&self, filter: QueryFilter<'_>) -> Result<Vec<Message>, StoreError> {
        let user = filter.user.unwrap_or("");
        let (ids, _total) = Self::query_messages(
            self,
            user,
            filter.mailbox_id,
            filter.text,
            filter.has_keyword.unwrap_or(0),
            filter.not_keyword.unwrap_or(0),
            true, // sort by internal_date descending — newest-first default
            filter.limit,
            filter.position,
        )
        .await?;

        // Inherent returns ids; fetch each. N+1 — acceptable for 2b, can be
        // optimized later with a bulk fetch.
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(msg) = MailboxStore::get_message(self, id).await? {
                out.push(msg);
            }
        }
        Ok(out)
    }

    async fn user_storage_bytes(&self, user: &str) -> Result<u64, StoreError> {
        // user_storage_usage returns u64 directly (no Result), wrap.
        Ok(Self::user_storage_usage(self, user).await)
    }
}
