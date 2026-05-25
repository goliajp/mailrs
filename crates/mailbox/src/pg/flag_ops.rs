use sqlx::PgPool;

use crate::pg::PgMailboxStore;
use crate::pg::helpers::row_to_message_meta;
use crate::types::{FlagAction, MessageMeta};

impl PgMailboxStore {
    /// Replace the flag bitmask on a single message (IMAP `STORE FLAGS`).
    /// Bumps the mailbox `highest_modseq` and returns the new value.
    pub async fn update_flags(
        &self,
        mailbox_id: i64,
        uid: u32,
        flags: u32,
    ) -> Result<u64, sqlx::Error> {
        let modseq = bump_modseq(&self.pool, mailbox_id).await?;
        sqlx::query(
            "UPDATE messages SET flags = $1, modseq = $4 WHERE mailbox_id = $2 AND uid = $3",
        )
        .bind(flags as i32)
        .bind(mailbox_id)
        .bind(uid as i32)
        .bind(modseq as i64)
        .execute(&self.pool)
        .await?;
        Ok(modseq)
    }

    /// OR the given bits into a message's flag bitmask (IMAP `STORE +FLAGS`).
    /// Bumps the mailbox `highest_modseq` and returns the new value.
    pub async fn add_flags(
        &self,
        mailbox_id: i64,
        uid: u32,
        flags: u32,
    ) -> Result<u64, sqlx::Error> {
        let modseq = bump_modseq(&self.pool, mailbox_id).await?;
        sqlx::query(
            "UPDATE messages SET flags = flags | $1, modseq = $4 WHERE mailbox_id = $2 AND uid = $3",
        )
        .bind(flags as i32)
        .bind(mailbox_id)
        .bind(uid as i32)
        .bind(modseq as i64)
        .execute(&self.pool)
        .await?;
        Ok(modseq)
    }

    /// AND-NOT the given bits out of a message's flag bitmask (IMAP
    /// `STORE -FLAGS`). Bumps the mailbox `highest_modseq` and returns
    /// the new value.
    pub async fn remove_flags(
        &self,
        mailbox_id: i64,
        uid: u32,
        flags: u32,
    ) -> Result<u64, sqlx::Error> {
        let modseq = bump_modseq(&self.pool, mailbox_id).await?;
        sqlx::query(
            "UPDATE messages SET flags = flags & ~$1, modseq = $4 WHERE mailbox_id = $2 AND uid = $3",
        )
        .bind(flags as i32)
        .bind(mailbox_id)
        .bind(uid as i32)
        .bind(modseq as i64)
        .execute(&self.pool)
        .await?;
        Ok(modseq)
    }

    /// conditionally update flags only if message modseq <= unchangedsince (RFC 7162 CONDSTORE)
    /// returns Ok(Some(new_modseq)) on success, Ok(None) if precondition failed
    pub async fn update_flags_if_unchanged(
        &self,
        mailbox_id: i64,
        uid: u32,
        flags: u32,
        action: FlagAction,
        unchangedsince: u64,
    ) -> Result<Option<u64>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        // check current modseq (lock row to prevent concurrent modification)
        let current: (i64,) = sqlx::query_as(
            "SELECT modseq FROM messages WHERE mailbox_id = $1 AND uid = $2 FOR UPDATE",
        )
        .bind(mailbox_id)
        .bind(uid as i32)
        .fetch_one(&mut *tx)
        .await?;

        if current.0 as u64 > unchangedsince {
            tx.rollback().await?;
            return Ok(None);
        }

        // bump modseq via RETURNING
        let row: (i64,) = sqlx::query_as(
            "UPDATE mailboxes SET highest_modseq = highest_modseq + 1
             WHERE id = $1 RETURNING highest_modseq",
        )
        .bind(mailbox_id)
        .fetch_one(&mut *tx)
        .await?;
        let modseq = row.0 as u64;

        match action {
            FlagAction::Set => {
                sqlx::query(
                    "UPDATE messages SET flags = $1, modseq = $4 WHERE mailbox_id = $2 AND uid = $3",
                )
                .bind(flags as i32)
                .bind(mailbox_id)
                .bind(uid as i32)
                .bind(modseq as i64)
                .execute(&mut *tx)
                .await?;
            }
            FlagAction::Add => {
                sqlx::query(
                    "UPDATE messages SET flags = flags | $1, modseq = $4 WHERE mailbox_id = $2 AND uid = $3",
                )
                .bind(flags as i32)
                .bind(mailbox_id)
                .bind(uid as i32)
                .bind(modseq as i64)
                .execute(&mut *tx)
                .await?;
            }
            FlagAction::Remove => {
                sqlx::query(
                    "UPDATE messages SET flags = flags & ~$1, modseq = $4 WHERE mailbox_id = $2 AND uid = $3",
                )
                .bind(flags as i32)
                .bind(mailbox_id)
                .bind(uid as i32)
                .bind(modseq as i64)
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
        Ok(Some(modseq))
    }

    /// list messages changed since a given modseq (for CONDSTORE FETCH CHANGEDSINCE)
    pub async fn list_messages_changed_since(
        &self,
        mailbox_id: i64,
        modseq: u64,
    ) -> Result<Vec<MessageMeta>, sqlx::Error> {
        let rows = sqlx::query_as::<_, (i64, i64, i32, String, String, String, String, i64, i32, i32, i64, String, String, String, i64)>(
            "SELECT id, mailbox_id, uid, maildir_id, sender, recipients, subject, date_epoch, size, flags, internal_date, message_id, in_reply_to, thread_id, modseq
             FROM messages WHERE mailbox_id = $1 AND modseq > $2 ORDER BY uid",
        )
        .bind(mailbox_id)
        .bind(modseq as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(row_to_message_meta).collect())
    }
}

/// bump highest_modseq for mailbox and return the new value
async fn bump_modseq(pool: &PgPool, mailbox_id: i64) -> Result<u64, sqlx::Error> {
    let row = sqlx::query_as::<_, (i64,)>(
        "UPDATE mailboxes SET highest_modseq = highest_modseq + 1
         WHERE id = $1 RETURNING highest_modseq",
    )
    .bind(mailbox_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0 as u64)
}
