//! Thread / conversation operations on [`PgMailboxStore`].
//!
//! Sub-modules:
//! - [`query`] — the per-thread rows every conversation view reads
//!   (`thread_rows`).
//! - [`mutate`] — flag/state mutations (mark read/unread, star, pin,
//!   archive, snooze, delete).
//! - [`filing`] — filing into a bucket, marking many as read, the sent list.
//! - [`holds`] — fraud verdicts and conversations held for review.
//! - [`backfill`] — the maildir-driven `backfill_threading` migration tool.
//!
//! This file owns the small lookups.

use crate::pg::PgMailboxStore;
use crate::pg::helpers::row_to_message_meta_from_row;
use crate::types::MessageMeta;

mod backfill;
mod filing;
mod holds;
mod mutate;
mod query;

pub use query::{Held, ThreadQuery};

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

    /// Subject of a thread's oldest message — the conversation's root
    /// topic, used by the Gmail subject rule (a reply only joins its
    /// ancestor's thread when the normalized subjects agree).
    pub async fn thread_root_subject(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT m.subject FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND m.thread_id = $2
             ORDER BY m.internal_date ASC LIMIT 1",
        )
        .bind(user)
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.0))
    }

    /// Apply the Gmail subject rule to a resolved thread id: when the
    /// resolve actually joined a known parent thread AND that thread's
    /// root subject disagrees with the message's, fall back to the
    /// message's own id (new conversation). Orphan/new paths pass
    /// through untouched.
    pub async fn apply_subject_gate(
        &self,
        user: &str,
        own_id: &str,
        subject: &str,
        parent_tid: Option<String>,
        resolved: String,
    ) -> String {
        let Some(tid) = parent_tid else {
            return resolved;
        };
        if resolved != tid {
            return resolved;
        }
        let root = self.thread_root_subject(user, &tid).await.ok().flatten();
        match root {
            Some(root_subject) if !crate::threading::same_topic(subject, &root_subject) => {
                own_id.to_string()
            }
            _ => resolved,
        }
    }

    /// Whether `user` holds a message in `thread_id`: who may act on the
    /// thread. Uses `idx_messages_thread`.
    pub async fn user_in_thread(&self, user: &str, thread_id: &str) -> Result<bool, sqlx::Error> {
        let (found,): (bool,) = sqlx::query_as(
            "SELECT EXISTS (SELECT 1 FROM messages m
                            JOIN mailboxes mb ON m.mailbox_id = mb.id
                            WHERE m.thread_id = $1 AND mb.user_address = $2)",
        )
        .bind(thread_id)
        .bind(user)
        .fetch_one(&self.pool)
        .await?;
        Ok(found)
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
}
