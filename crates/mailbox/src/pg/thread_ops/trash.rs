//! The Trash, per reader: a row in `trashed_conversations` keeps the
//! conversation out of every other list until it is restored, deleted for
//! good, or the purge reaches it 30 days on.

use crate::pg::PgMailboxStore;
use crate::pg::helpers::row_to_message_meta;
use crate::types::MessageMeta;

impl PgMailboxStore {
    /// Move `thread_id` to `user`'s Trash at `now`.
    pub async fn trash_thread(
        &self,
        user: &str,
        thread_id: &str,
        now: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO trashed_conversations (account_address, thread_id, trashed_at)
             VALUES ($1, $2, $3)
             ON CONFLICT (account_address, thread_id) DO UPDATE SET trashed_at = $3",
        )
        .bind(user)
        .bind(thread_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Take `thread_id` back out of `user`'s Trash.
    pub async fn untrash_thread(&self, user: &str, thread_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "DELETE FROM trashed_conversations WHERE account_address = $1 AND thread_id = $2",
        )
        .bind(user)
        .bind(thread_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// What IMAP's `Trash` shows: every message of `user`'s conversations
    /// in the Trash and every message filed in a mailbox named Trash, by
    /// id; and when the latest conversation went there (0 for none).
    pub async fn trashed_messages(
        &self,
        user: &str,
    ) -> Result<(Vec<MessageMeta>, i64), sqlx::Error> {
        let rows = sqlx::query_as::<_, (i64, i64, i32, String, String, String, String, i64, i32, i32, i64, String, String, String, i64)>(
            "SELECT m.id, m.mailbox_id, m.uid, m.maildir_id, m.sender, m.recipients, m.subject, m.date_epoch, m.size, m.flags, m.internal_date, m.message_id, m.in_reply_to, m.thread_id, m.modseq
               FROM messages m JOIN mailboxes mb ON mb.id = m.mailbox_id
              WHERE mb.user_address = $1
                AND (LOWER(mb.name) = 'trash'
                     OR m.thread_id IN (SELECT thread_id FROM trashed_conversations
                                         WHERE account_address = $1))
              ORDER BY m.id",
        )
        .bind(user)
        .fetch_all(&self.pool)
        .await?;
        let (latest,): (i64,) = sqlx::query_as(
            "SELECT COALESCE(MAX(trashed_at), 0) FROM trashed_conversations WHERE account_address = $1",
        )
        .bind(user)
        .fetch_one(&self.pool)
        .await?;
        Ok((rows.into_iter().map(row_to_message_meta).collect(), latest))
    }

    /// Every conversation in `user`'s Trash.
    pub async fn trashed_threads(&self, user: &str) -> Result<Vec<String>, sqlx::Error> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT thread_id FROM trashed_conversations WHERE account_address = $1",
        )
        .bind(user)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(t,)| t).collect())
    }

    /// Up to `limit` `(user, thread id)` trashed at or before `cutoff`.
    pub async fn trashed_before(
        &self,
        cutoff: i64,
        limit: i64,
    ) -> Result<Vec<(String, String)>, sqlx::Error> {
        sqlx::query_as(
            "SELECT account_address, thread_id FROM trashed_conversations
              WHERE trashed_at <= $1 ORDER BY trashed_at LIMIT $2",
        )
        .bind(cutoff)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }
}
