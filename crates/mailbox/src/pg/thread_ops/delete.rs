//! Deleting a reader's copy of a conversation, with what is kept beside
//! its messages: its Trash entry, labels, hold facts and ledger.

use crate::pg::PgMailboxStore;

impl PgMailboxStore {
    /// delete all mailbox entries for a thread belonging to a user
    /// messages table rows are left intact (other users may share them)
    /// returns list of (user_address, maildir_id) for physical file cleanup
    pub async fn delete_thread(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        // collect maildir_ids to delete from disk
        let maildir_ids: Vec<(String,)> = sqlx::query_as(
            "SELECT m.maildir_id FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE m.thread_id = $1 AND mb.user_address = $2",
        )
        .bind(thread_id)
        .bind(user)
        .fetch_all(&self.pool)
        .await?;

        let mids: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT m.message_id FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE m.thread_id = $1 AND mb.user_address = $2",
        )
        .bind(thread_id)
        .bind(user)
        .fetch_all(&self.pool)
        .await?;

        // remove from messages table for this user's mailboxes
        sqlx::query(
            "DELETE FROM messages
             WHERE thread_id = $1
               AND mailbox_id IN (SELECT id FROM mailboxes WHERE user_address = $2)",
        )
        .bind(thread_id)
        .bind(user)
        .execute(&self.pool)
        .await?;
        // nothing left to keep in the Trash: the purge would find this row
        // again on every pass
        self.untrash_thread(user, thread_id).await?;
        self.drop_thread_labels(user, thread_id).await?;
        self.drop_hold_facts(user, thread_id).await?;
        let mids: Vec<String> = mids.into_iter().map(|(m,)| m).collect();
        self.forget_ledger_of_gone(user, &mids).await?;

        Ok(maildir_ids.into_iter().map(|(id,)| id).collect())
    }
}
