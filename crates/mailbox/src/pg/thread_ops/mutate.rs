//! Thread-level mutation operations: read/unread flags, star/pin/archive,
//! snooze/unsnooze, delete, and email-analysis dismissal.

use crate::pg::PgMailboxStore;
use crate::types::{FLAG_FLAGGED, FLAG_SEEN};

impl PgMailboxStore {
    /// mark all messages in a thread as read
    /// when `domains` is provided, marks read across all accounts in those domains
    pub async fn mark_thread_read(
        &self,
        user: &str,
        thread_id: &str,
        domains: Option<&[String]>,
    ) -> Result<u32, sqlx::Error> {
        // determine user filter and param count
        let (user_filter, extra_params) = if let Some(doms) = domains.filter(|d| !d.is_empty()) {
            let placeholders: Vec<String> = doms
                .iter()
                .enumerate()
                .map(|(i, _)| format!("${}", i + 3))
                .collect();
            (
                format!(
                    "user_address IN (SELECT address FROM accounts WHERE domain IN ({}))",
                    placeholders.join(",")
                ),
                doms.len(),
            )
        } else {
            ("user_address = $3".to_string(), 1usize)
        };
        let modseq_idx = 3 + extra_params;

        // bump highest_modseq for all affected mailboxes
        let bump_sql = format!(
            "UPDATE mailboxes SET highest_modseq = highest_modseq + 1
             WHERE id IN (
                SELECT DISTINCT mailbox_id FROM messages
                WHERE thread_id = $1 AND (flags & $2) = 0
                  AND mailbox_id IN (SELECT id FROM mailboxes WHERE {user_filter})
             )"
        );
        let mut q = sqlx::query(&bump_sql)
            .bind(thread_id)
            .bind(FLAG_SEEN as i32);
        if let Some(doms) = domains.filter(|d| !d.is_empty()) {
            for d in doms {
                q = q.bind(d);
            }
        } else {
            q = q.bind(user);
        }
        q.execute(&self.pool).await?;

        // get new modseq (use user's own mailbox modseq as baseline)
        let new_modseq: (i64,) = sqlx::query_as(
            "SELECT COALESCE(MAX(highest_modseq), 0) FROM mailboxes WHERE user_address = $1",
        )
        .bind(user)
        .fetch_one(&self.pool)
        .await?;

        // mark messages as read
        let update_sql = format!(
            "UPDATE messages SET flags = flags | $1, modseq = ${modseq_idx}
             WHERE thread_id = $2 AND (flags & $1) = 0
               AND mailbox_id IN (SELECT id FROM mailboxes WHERE {user_filter})"
        );
        let mut q = sqlx::query(&update_sql)
            .bind(FLAG_SEEN as i32)
            .bind(thread_id);
        if let Some(doms) = domains.filter(|d| !d.is_empty()) {
            for d in doms {
                q = q.bind(d);
            }
        } else {
            q = q.bind(user);
        }
        q = q.bind(new_modseq.0);
        let result = q.execute(&self.pool).await?;

        Ok(result.rows_affected() as u32)
    }

    /// mark only the latest message in a thread as unread for a user
    pub async fn mark_thread_unread(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<u32, sqlx::Error> {
        // bump modseq on affected mailboxes
        sqlx::query(
            "UPDATE mailboxes SET highest_modseq = highest_modseq + 1
             WHERE id IN (
                SELECT DISTINCT mailbox_id FROM messages
                WHERE thread_id = $1 AND (flags & $2) != 0
                  AND mailbox_id IN (SELECT id FROM mailboxes WHERE user_address = $3)
             )",
        )
        .bind(thread_id)
        .bind(FLAG_SEEN as i32)
        .bind(user)
        .execute(&self.pool)
        .await?;

        let new_modseq: (i64,) = sqlx::query_as(
            "SELECT COALESCE(MAX(highest_modseq), 0) FROM mailboxes WHERE user_address = $1",
        )
        .bind(user)
        .fetch_one(&self.pool)
        .await?;

        // clear seen flag only on the most recent message in the thread
        let result = sqlx::query(
            "UPDATE messages SET flags = flags & ~$1, modseq = $4
             WHERE id = (
                SELECT m.id FROM messages m
                JOIN mailboxes mb ON m.mailbox_id = mb.id
                WHERE m.thread_id = $2 AND mb.user_address = $3
                ORDER BY m.internal_date DESC
                LIMIT 1
             )",
        )
        .bind(FLAG_SEEN as i32)
        .bind(thread_id)
        .bind(user)
        .bind(new_modseq.0)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as u32)
    }

    /// set FLAG_FLAGGED on all messages in a thread for the user
    pub async fn star_thread(&self, user: &str, thread_id: &str) -> Result<u32, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE messages SET flags = flags | $1
             WHERE thread_id = $2
               AND mailbox_id IN (SELECT id FROM mailboxes WHERE user_address = $3)",
        )
        .bind(FLAG_FLAGGED as i32)
        .bind(thread_id)
        .bind(user)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as u32)
    }

    /// clear FLAG_FLAGGED on all messages in a thread for the user
    pub async fn unstar_thread(&self, user: &str, thread_id: &str) -> Result<u32, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE messages SET flags = flags & ~$1
             WHERE thread_id = $2
               AND mailbox_id IN (SELECT id FROM mailboxes WHERE user_address = $3)",
        )
        .bind(FLAG_FLAGGED as i32)
        .bind(thread_id)
        .bind(user)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as u32)
    }

    /// set pinned=true on all messages in a thread for the user
    pub async fn pin_thread(&self, user: &str, thread_id: &str) -> Result<u32, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE messages SET pinned = true
             WHERE thread_id = $1
               AND mailbox_id IN (SELECT id FROM mailboxes WHERE user_address = $2)",
        )
        .bind(thread_id)
        .bind(user)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as u32)
    }

    /// set pinned=false on all messages in a thread for the user
    pub async fn unpin_thread(&self, user: &str, thread_id: &str) -> Result<u32, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE messages SET pinned = false
             WHERE thread_id = $1
               AND mailbox_id IN (SELECT id FROM mailboxes WHERE user_address = $2)",
        )
        .bind(thread_id)
        .bind(user)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as u32)
    }

    /// set archived=true on all messages in a thread for the user
    pub async fn archive_thread(&self, user: &str, thread_id: &str) -> Result<u32, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE messages SET archived = true
             WHERE thread_id = $1
               AND mailbox_id IN (SELECT id FROM mailboxes WHERE user_address = $2)",
        )
        .bind(thread_id)
        .bind(user)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as u32)
    }

    /// set archived=false on all messages in a thread for the user
    pub async fn unarchive_thread(&self, user: &str, thread_id: &str) -> Result<u32, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE messages SET archived = false
             WHERE thread_id = $1
               AND mailbox_id IN (SELECT id FROM mailboxes WHERE user_address = $2)",
        )
        .bind(thread_id)
        .bind(user)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as u32)
    }

    /// v2.9 triage — the maildir ids of every message in a thread for
    /// the user. Used to read the raw messages (for classifier
    /// training) at mark-action time. Newest first.
    pub async fn get_thread_maildir_ids(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT m.maildir_id
             FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE m.thread_id = $1 AND mb.user_address = $2 AND m.maildir_id != ''
             ORDER BY m.internal_date DESC",
        )
        .bind(thread_id)
        .bind(user)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// v2.9 triage — force a thread into a bucket by stamping the
    /// `email_analysis.category` of every message in the thread to
    /// `category` ∈ {"inbox","notification","promotion","spam"}. This is
    /// the monolith/spg analog of fastcore's `set_bucket` (which mutates
    /// the kevy folder zsets); here the bucket is derived from the
    /// category (see `list_conversations` folder filter), so stamping
    /// category IS the move. Ingest does not create `email_analysis`
    /// rows, so this UPSERTs. Returns the number of messages affected.
    pub async fn set_thread_bucket(
        &self,
        user: &str,
        thread_id: &str,
        category: &str,
    ) -> Result<u32, sqlx::Error> {
        let result = sqlx::query(
            "INSERT INTO email_analysis (message_id, category)
             SELECT m.id, $3
             FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE m.thread_id = $1 AND mb.user_address = $2
             ON CONFLICT (message_id) DO UPDATE SET category = EXCLUDED.category",
        )
        .bind(thread_id)
        .bind(user)
        .bind(category)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as u32)
    }

    /// snooze a conversation until a given time
    pub async fn snooze_thread(
        &self,
        user: &str,
        thread_id: &str,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO snoozed_conversations (thread_id, account_address, snoozed_until)
             VALUES ($1, $2, $3)
             ON CONFLICT (thread_id, account_address) DO UPDATE SET snoozed_until = $3",
        )
        .bind(thread_id)
        .bind(user)
        .bind(until)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// unsnooze a conversation
    pub async fn unsnooze_thread(&self, user: &str, thread_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "DELETE FROM snoozed_conversations WHERE thread_id = $1 AND account_address = $2",
        )
        .bind(thread_id)
        .bind(user)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

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

        Ok(maildir_ids.into_iter().map(|(id,)| id).collect())
    }

    /// dismiss action for all messages in a thread: clear requires_action and reverse importance boost
    pub async fn dismiss_thread_action(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<u32, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE email_analysis SET requires_action = false
             WHERE message_id IN (
               SELECT m.id FROM messages m
               JOIN mailboxes mb ON m.mailbox_id = mb.id
               WHERE m.thread_id = $1 AND mb.user_address = $2
             ) AND requires_action = true",
        )
        .bind(thread_id)
        .bind(user)
        .execute(&self.pool)
        .await?;

        let affected = result.rows_affected() as u32;

        if affected > 0 {
            sqlx::query(
                "UPDATE messages SET
                   importance_score = GREATEST(-0.5, importance_score - 0.2),
                   importance_level = CASE
                     WHEN GREATEST(-0.5, importance_score - 0.2) >= 0.8 THEN 'critical'
                     WHEN GREATEST(-0.5, importance_score - 0.2) >= 0.5 THEN 'important'
                     WHEN GREATEST(-0.5, importance_score - 0.2) >= 0.2 THEN 'normal'
                     WHEN GREATEST(-0.5, importance_score - 0.2) >= 0.0 THEN 'low'
                     ELSE 'noise'
                   END
                 WHERE thread_id = $1
                   AND mailbox_id IN (SELECT id FROM mailboxes WHERE user_address = $2)",
            )
            .bind(thread_id)
            .bind(user)
            .execute(&self.pool)
            .await?;
        }

        Ok(affected)
    }
}
