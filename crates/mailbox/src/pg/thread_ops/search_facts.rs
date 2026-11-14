//! What the search operators read of a conversation's messages.

use crate::pg::PgMailboxStore;
use crate::query::MessageFacts;

impl PgMailboxStore {
    /// Each of `user`'s messages in the thread, oldest first, as the
    /// search operators read them.
    pub async fn message_facts(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Vec<MessageFacts>, sqlx::Error> {
        let rows: Vec<(String, String, String, Option<i32>)> = sqlx::query_as(
            "SELECT m.sender, m.recipients, m.subject, m.attachments
               FROM messages m JOIN mailboxes mb ON mb.id = m.mailbox_id
              WHERE m.thread_id = $1 AND mb.user_address = $2
              ORDER BY m.date_epoch",
        )
        .bind(thread_id)
        .bind(user)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(sender, recipients, subject, attachments)| MessageFacts {
                sender,
                recipients,
                subject,
                attachments: attachments.map(|n| n.max(0) as u32),
            })
            .collect())
    }
}

impl PgMailboxStore {
    /// Attachments counted for each of these messages, by row id; a message
    /// stored before the count was kept is absent.
    pub async fn attachment_counts(
        &self,
        ids: &[i64],
    ) -> Result<std::collections::HashMap<i64, u32>, sqlx::Error> {
        let rows: Vec<(i64, i32)> = sqlx::query_as(
            "SELECT id, attachments FROM messages WHERE id = ANY($1) AND attachments IS NOT NULL",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, n)| (id, n.max(0) as u32))
            .collect())
    }

    /// The invitation method (`REQUEST`, `CANCEL`, ...) of each message in
    /// `ids` that carries one, which the message reads do not select.
    pub async fn invite_methods(
        &self,
        ids: &[i64],
    ) -> Result<std::collections::HashMap<i64, String>, sqlx::Error> {
        let rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT id, invite_method FROM messages \
             WHERE id = ANY($1) AND invite_method IS NOT NULL AND invite_method <> ''",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().collect())
    }
}
