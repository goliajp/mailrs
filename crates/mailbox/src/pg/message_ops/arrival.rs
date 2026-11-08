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
    /// Attachments in the message, or `None` when not counted.
    pub attachments: Option<u32>,
    /// What delivery recorded for the sectioned inbox, or `None` for a
    /// message carried over without them (its columns stay NULL).
    pub section: Option<crate::section::MessageFacts>,
    /// The sender's bare address, lowercased: what an acceptance matches.
    pub from_addr: &'a str,
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
        let section = facts.section;
        sqlx::query(
            "UPDATE messages SET snippet = $1, sender_trust = $2, attachments = $3,
                    direct = $4, from_person = $5, first_contact = $6, to_others = $7,
                    from_addr = $8, invite = $9, mutual = $10
              WHERE id = $11",
        )
        .bind(facts.snippet)
        .bind(facts.sender_trust)
        .bind(facts.attachments.map(|n| n as i32))
        .bind(section.map(|s| s.direct))
        .bind(section.map(|s| s.from_person))
        .bind(section.map(|s| s.first_contact))
        .bind(section.map(|s| s.to_others))
        .bind(facts.from_addr)
        .bind(section.map(|s| s.invite))
        .bind(section.map(|s| s.mutual))
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

    /// Whether `user` has written in the thread, as the section reads it: a
    /// message of theirs with recorded facts (`s_own_at` in the sections'
    /// aggregates).
    pub async fn reader_wrote_in(&self, user: &str, thread_id: &str) -> Result<bool, sqlx::Error> {
        let (wrote,): (bool,) = sqlx::query_as(
            "SELECT EXISTS (SELECT 1 FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
              WHERE mb.user_address = $1 AND m.thread_id = $2 AND mb.name = 'Sent'
                AND m.direct IS NOT NULL)",
        )
        .bind(user)
        .bind(thread_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(wrote)
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

impl PgMailboxStore {
    /// Record that `user` accepted each of `addresses` at `at`: a fact per
    /// sender, appended; one already there keeps its first time.
    pub async fn accept_senders(
        &self,
        user: &str,
        addresses: &[String],
        at: i64,
    ) -> Result<(), sqlx::Error> {
        for address in addresses {
            sqlx::query(
                "INSERT INTO accepted_senders (account_address, address, accepted_at)
                 VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            )
            .bind(user)
            .bind(address)
            .bind(at)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }
}
