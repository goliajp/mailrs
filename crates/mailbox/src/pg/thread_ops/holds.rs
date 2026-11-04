//! Fraud verdicts and the conversations held for review because of them.
//!
//! A verdict is recorded once, when the receiver's verdict arrives with the
//! spooled message; it is a fact and is never rewritten. A hold is per
//! account: releasing a conversation for one reader leaves it held for any
//! other. A held conversation is out of every list, count and search (the
//! thread rows carry `quarantined`) until released.

use crate::pg::PgMailboxStore;
use crate::pg::helpers::row_to_message_meta;
use crate::types::MessageMeta;

impl PgMailboxStore {
    /// Record the verdict for `message_id`. The first one stands.
    pub async fn record_fraud_verdict(
        &self,
        message_id: &str,
        verdict: &serde_json::Value,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO fraud_verdicts (message_id, verdict) VALUES ($1, CAST($2 AS JSONB))
             ON CONFLICT (message_id) DO NOTHING",
        )
        .bind(message_id)
        .bind(verdict.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The verdict recorded for `message_id`, if any.
    pub async fn fraud_verdict(
        &self,
        message_id: &str,
    ) -> Result<Option<serde_json::Value>, sqlx::Error> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT CAST(verdict AS TEXT) FROM fraud_verdicts WHERE message_id = $1",
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|(v,)| serde_json::from_str(&v))
            .transpose()
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))
    }

    /// Record why `user`'s copy of `message_id` was filed where it was. The
    /// first one stands, as for the verdict.
    pub async fn record_message_placement(
        &self,
        user: &str,
        message_id: &str,
        placement: &serde_json::Value,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO message_placements (account_address, message_id, placement)
             VALUES ($1, $2, CAST($3 AS JSONB))
             ON CONFLICT (account_address, message_id) DO NOTHING",
        )
        .bind(user)
        .bind(message_id)
        .bind(placement.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Why `user`'s copy of `message_id` was filed where it was, if anything
    /// but the plain inbox filed it.
    pub async fn message_placement(
        &self,
        user: &str,
        message_id: &str,
    ) -> Result<Option<serde_json::Value>, sqlx::Error> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT CAST(placement AS TEXT) FROM message_placements
              WHERE account_address = $1 AND message_id = $2",
        )
        .bind(user)
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|(v,)| serde_json::from_str(&v))
            .transpose()
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))
    }

    /// The messages of `user`'s held conversations, wherever they are filed.
    pub async fn held_messages(&self, user: &str) -> Result<Vec<MessageMeta>, sqlx::Error> {
        let rows = sqlx::query_as::<_, (i64, i64, i32, String, String, String, String, i64, i32, i32, i64, String, String, String, i64)>(
            "SELECT m.id, m.mailbox_id, m.uid, m.maildir_id, m.sender, m.recipients, m.subject, m.date_epoch, m.size, m.flags, m.internal_date, m.message_id, m.in_reply_to, m.thread_id, m.modseq
               FROM messages m JOIN mailboxes mb ON mb.id = m.mailbox_id
              WHERE mb.user_address = $1
                AND m.thread_id IN (SELECT thread_id FROM quarantine_holds
                                     WHERE account_address = $1)
              ORDER BY m.id",
        )
        .bind(user)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_message_meta).collect())
    }

    /// Hold `thread_id` for `user`. Returns whether it was not held before.
    pub async fn hold_thread(&self, user: &str, thread_id: &str) -> Result<bool, sqlx::Error> {
        let done = sqlx::query(
            "INSERT INTO quarantine_holds (account_address, thread_id) VALUES ($1, $2)
             ON CONFLICT (account_address, thread_id) DO NOTHING",
        )
        .bind(user)
        .bind(thread_id)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() > 0)
    }

    /// Release `thread_id` for `user`. The verdict stays; it was true of
    /// the message when it arrived. Returns how many holds went.
    pub async fn release_thread(&self, user: &str, thread_id: &str) -> Result<u64, sqlx::Error> {
        let done = sqlx::query(
            "DELETE FROM quarantine_holds WHERE account_address = $1 AND thread_id = $2",
        )
        .bind(user)
        .bind(thread_id)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected())
    }

    /// Append one release of `thread_id` for `user`. Never updated.
    pub async fn record_release(
        &self,
        user: &str,
        thread_id: &str,
        fact: &ReleaseFact,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO release_facts
                 (account_address, thread_id, released_at, source, actor, rules_version)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(user)
        .bind(thread_id)
        .bind(fact.at)
        .bind(&fact.source)
        .bind(&fact.actor)
        .bind(&fact.rules_version)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Every release of `thread_id` for `user`, oldest first.
    pub async fn releases(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Vec<ReleaseFact>, sqlx::Error> {
        let rows: Vec<(i64, String, String, String)> = sqlx::query_as(
            "SELECT released_at, source, actor, rules_version FROM release_facts
              WHERE account_address = $1 AND thread_id = $2
              ORDER BY id",
        )
        .bind(user)
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(at, source, actor, rules_version)| ReleaseFact {
                at,
                source,
                actor,
                rules_version,
            })
            .collect())
    }
}

/// One release of a held conversation, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseFact {
    /// Unix seconds.
    pub at: i64,
    /// `person`, `batch` or `rejudge`.
    pub source: String,
    /// The reader or administrator, or `system`.
    pub actor: String,
    /// The rule pack that let it go; empty unless a rule change did.
    pub rules_version: String,
}
