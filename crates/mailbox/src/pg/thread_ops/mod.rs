//! Thread / conversation operations on [`PgMailboxStore`].
//!
//! Sub-modules:
//! - [`query`] — conversation-list SELECTs (`list_conversations`,
//!   `get_conversations_by_thread_ids`).
//! - [`mutate`] — flag/state mutations (mark read/unread, star, pin,
//!   archive, snooze, delete, dismiss-action).
//!
//! This file owns the small lookups + the maildir-driven
//! `backfill_threading` migration tool.

use crate::pg::PgMailboxStore;
use crate::pg::helpers::{read_raw_from_maildir, row_to_message_meta_from_row};
use crate::threading;
use crate::types::MessageMeta;

mod mutate;
mod query;

impl PgMailboxStore {
    /// look up the thread_id of a message by its message_id (across all user's mailboxes)
    pub async fn find_thread_id_by_message_id(
        &self,
        user: &str,
        message_id: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT m.thread_id FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND m.message_id = $2 AND m.thread_id != ''
             LIMIT 1",
        )
        .bind(user)
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.0))
    }

    /// list all messages in a thread (deduplicated by message_id)
    /// when `domains` is Some, query across all accounts in those domains
    pub async fn list_thread_messages(
        &self,
        user: &str,
        thread_id: &str,
        domains: Option<&[String]>,
    ) -> Result<Vec<MessageMeta>, sqlx::Error> {
        // deduplicate: same email may exist in both INBOX and Sent
        let (user_filter, user_filter_inner) = if let Some(doms) = domains {
            if !doms.is_empty() {
                let placeholders: Vec<String> = doms
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("${}", i + 3))
                    .collect();
                let f = format!(
                    "mb.user_address IN (SELECT address FROM accounts WHERE domain IN ({}))",
                    placeholders.join(",")
                );
                let f2 = format!(
                    "mb2.user_address IN (SELECT address FROM accounts WHERE domain IN ({}))",
                    placeholders.join(",")
                );
                (f, f2)
            } else {
                (
                    "mb.user_address = $1".to_string(),
                    "mb2.user_address = $1".to_string(),
                )
            }
        } else {
            (
                "mb.user_address = $1".to_string(),
                "mb2.user_address = $1".to_string(),
            )
        };

        let sql = format!(
            "SELECT m.id, m.mailbox_id, m.uid, m.maildir_id, m.sender, m.recipients, m.subject,
                    m.date_epoch, m.size, m.flags, m.internal_date, m.message_id, m.in_reply_to, m.thread_id, m.modseq,
                    mb.user_address,
                    COALESCE(m.importance_level, 'normal'), COALESCE(m.importance_score, 0.0),
                    COALESCE(m.is_bulk_sender, false), COALESCE(m.has_tracking_pixel, false),
                    m.new_content
             FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE {user_filter} AND m.thread_id = $2
               AND m.id = (
                 SELECT MIN(m2.id) FROM messages m2
                 JOIN mailboxes mb2 ON m2.mailbox_id = mb2.id
                 WHERE {user_filter_inner}
                   AND CASE WHEN m.message_id != '' THEN m2.message_id = m.message_id
                            ELSE m2.id = m.id END
               )
             ORDER BY m.internal_date ASC"
        );

        let mut query = sqlx::query(&sql).bind(user).bind(thread_id);

        if let Some(doms) = domains
            && !doms.is_empty()
        {
            for d in doms {
                query = query.bind(d);
            }
        }

        let rows = query.fetch_all(&self.pool).await?;

        Ok(rows.into_iter().map(row_to_message_meta_from_row).collect())
    }

    /// get all message-ids in the thread that contains the given message_id,
    /// ordered by date (for building RFC 5322 References header)
    pub async fn get_thread_references(
        &self,
        user: &str,
        message_id: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        // first find the thread_id
        let thread_id_row = sqlx::query_as::<_, (String,)>(
            "SELECT m.thread_id FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND m.message_id = $2 AND m.thread_id != ''
             LIMIT 1",
        )
        .bind(user)
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;

        let thread_id = match thread_id_row {
            Some(r) => r.0,
            None => return Ok(vec![message_id.to_string()]),
        };

        // get all distinct message_ids in this thread, ordered by date
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT DISTINCT m.message_id
             FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND m.thread_id = $2 AND m.message_id != ''
             ORDER BY m.message_id",
        )
        .bind(user)
        .bind(&thread_id)
        .fetch_all(&self.pool)
        .await?;

        let ids: Vec<String> = rows.into_iter().map(|r| r.0).collect();
        if ids.is_empty() {
            Ok(vec![message_id.to_string()])
        } else {
            Ok(ids)
        }
    }

    /// get the message_id of the last message in a thread (by internal_date)
    pub async fn get_last_message_id_in_thread(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT m.message_id FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND m.thread_id = $2 AND m.message_id != ''
             ORDER BY m.internal_date DESC
             LIMIT 1",
        )
        .bind(user)
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.0))
    }

    /// get all message_ids in a thread ordered by date (for References header)
    pub async fn get_thread_message_ids(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT DISTINCT m.message_id
             FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND m.thread_id = $2 AND m.message_id != ''
             ORDER BY m.message_id",
        )
        .bind(user)
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// backfill threading data for messages that have empty thread_id
    /// reads raw bytes from maildir to extract Message-ID/In-Reply-To
    pub async fn backfill_threading(&self, maildir_root: &str) -> u32 {
        // find all messages missing thread_id
        let entries = sqlx::query_as::<_, (i64, String, String)>(
            "SELECT m.id, m.maildir_id, mb.user_address
             FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE m.thread_id = ''",
        )
        .fetch_all(&self.pool)
        .await;

        let entries = match entries {
            Ok(e) => e,
            Err(_) => return 0,
        };

        let mut count = 0u32;
        for (id, maildir_id, user) in &entries {
            let raw = read_raw_from_maildir(maildir_root, user, maildir_id);
            let Some(data) = raw else { continue };

            let msg_id = threading::extract_message_id(&data);
            if msg_id.is_empty() {
                continue;
            }
            let in_reply_to = threading::extract_in_reply_to(&data);

            // look up parent thread_id
            let parent_tid: Option<String> = if !in_reply_to.is_empty() {
                sqlx::query_as::<_, (String,)>(
                    "SELECT m.thread_id FROM messages m
                     JOIN mailboxes mb ON m.mailbox_id = mb.id
                     WHERE mb.user_address = $1 AND m.message_id = $2 AND m.thread_id != ''
                     LIMIT 1",
                )
                .bind(user)
                .bind(&in_reply_to)
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten()
                .map(|r| r.0)
            } else {
                None
            };

            let thread_id =
                threading::resolve_thread_id(&msg_id, &in_reply_to, |_| parent_tid.clone());

            let _ = sqlx::query(
                "UPDATE messages SET message_id = $1, in_reply_to = $2, thread_id = $3 WHERE id = $4",
            )
            .bind(&msg_id)
            .bind(&in_reply_to)
            .bind(&thread_id)
            .bind(id)
            .execute(&self.pool)
            .await;

            count += 1;
        }
        count
    }
}
