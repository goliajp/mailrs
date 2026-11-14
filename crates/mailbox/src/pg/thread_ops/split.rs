//! Moving a message out of its conversation.

use crate::pg::PgMailboxStore;

impl PgMailboxStore {
    /// Move `user`'s message `message_id` into a conversation of its own,
    /// named by its Message-ID: the repair for a reply joined to the wrong
    /// conversation. Every copy `user` holds moves; other readers' copies
    /// stay where they are. `None` when `user` holds no such message.
    pub async fn split_message(
        &self,
        user: &str,
        message_id: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let moved: Vec<(i64,)> = sqlx::query_as(
            "UPDATE messages SET thread_id = $2
             FROM mailboxes
             WHERE messages.mailbox_id = mailboxes.id
               AND mailboxes.user_address = $1
               AND messages.message_id = $2
             RETURNING messages.id",
        )
        .bind(user)
        .bind(message_id)
        .fetch_all(&self.pool)
        .await?;
        Ok((!moved.is_empty()).then(|| message_id.to_string()))
    }
}
