//! The Trash, per reader: a row in `trashed_conversations` keeps the
//! conversation out of every other list until it is restored, deleted for
//! good, or the purge reaches it 30 days on.

use crate::pg::PgMailboxStore;

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
