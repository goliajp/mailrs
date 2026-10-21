//! A delivered message's row, and the facts delivery records beside it.

use crate::pg::PgMailboxStore;
use crate::store::{MailboxStore, StoreError};
use crate::types::{InsertMessage, Inserted};

/// What delivery knows about a message beyond its headers.
#[derive(Debug, Clone, Copy)]
pub struct ArrivalFacts<'a> {
    /// The list's preview line.
    pub snippet: &'a str,
    /// The triage category the views file the conversation under.
    pub category: &'a str,
    /// The sender-authentication verdict, or `""` for none.
    pub sender_trust: &'a str,
}

impl PgMailboxStore {
    /// Insert the message row with its preview, sender verdict and
    /// category. Re-delivering the same maildir file into the same mailbox
    /// finds the row already there.
    pub async fn record_arrival(
        &self,
        input: InsertMessage<'_>,
        facts: ArrivalFacts<'_>,
    ) -> Result<Inserted, StoreError> {
        let inserted = MailboxStore::insert_message(self, input).await?;
        sqlx::query("UPDATE messages SET snippet = $1, sender_trust = $2 WHERE id = $3")
            .bind(facts.snippet)
            .bind(facts.sender_trust)
            .bind(inserted.id)
            .execute(&self.pool)
            .await?;
        sqlx::query(
            "INSERT INTO email_analysis (message_id, category) VALUES ($1, $2) \
             ON CONFLICT (message_id) DO UPDATE SET category = EXCLUDED.category",
        )
        .bind(inserted.id)
        .bind(facts.category)
        .execute(&self.pool)
        .await?;
        Ok(inserted)
    }

    /// The subject a conversation's row shows: its newest message the user
    /// did not send, or the first one when they sent them all.
    pub async fn thread_display_subject(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let inbound: Option<(String,)> = sqlx::query_as(
            "SELECT m.subject FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
              WHERE mb.user_address = $1 AND m.thread_id = $2 AND mb.name != 'Sent'
              ORDER BY m.internal_date DESC, m.id DESC LIMIT 1",
        )
        .bind(user)
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await?;
        if inbound.is_some() {
            return Ok(inbound.map(|(s,)| s));
        }
        self.thread_root_subject(user, thread_id).await
    }
}
