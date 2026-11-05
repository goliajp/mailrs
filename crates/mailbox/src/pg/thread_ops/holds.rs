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

    /// Hold `thread_id` for `user`, and append the hold fact when it was not
    /// held before: `source` (`ingest` or `report`), the rule pack, and the
    /// reader's messages unread at this moment. What is read is left as it
    /// is. Returns whether it was not held before.
    pub async fn hold_thread(
        &self,
        user: &str,
        thread_id: &str,
        source: &str,
        rules_version: &str,
    ) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let done = sqlx::query(
            "INSERT INTO quarantine_holds (account_address, thread_id) VALUES ($1, $2)
             ON CONFLICT (account_address, thread_id) DO NOTHING",
        )
        .bind(user)
        .bind(thread_id)
        .execute(&mut *tx)
        .await?;
        if done.rows_affected() == 0 {
            tx.commit().await?;
            return Ok(false);
        }
        let unread: Vec<(String,)> = sqlx::query_as(
            "SELECT m.message_id FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
              WHERE m.thread_id = $1 AND mb.user_address = $2 AND (m.flags & $3) = 0
              ORDER BY m.id",
        )
        .bind(thread_id)
        .bind(user)
        .bind(crate::types::FLAG_SEEN as i32)
        .fetch_all(&mut *tx)
        .await?;
        let mids: Vec<String> = unread.into_iter().map(|(m,)| m).collect();
        let mids = serde_json::to_string(&mids).map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
        sqlx::query(
            "INSERT INTO hold_facts
                 (account_address, thread_id, held_at, source, rules_version, unread_mids)
             VALUES ($1, $2, $3, $4, $5, CAST($6 AS JSONB))",
        )
        .bind(user)
        .bind(thread_id)
        .bind(chrono::Utc::now().timestamp())
        .bind(source)
        .bind(rules_version)
        .bind(mids)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Whether `thread_id` is held for `user` now.
    pub async fn is_held(&self, user: &str, thread_id: &str) -> Result<bool, sqlx::Error> {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM quarantine_holds WHERE account_address = $1 AND thread_id = $2",
        )
        .bind(user)
        .bind(thread_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(n > 0)
    }

    /// Every hold of `thread_id` for `user`, oldest first.
    pub async fn holds(&self, user: &str, thread_id: &str) -> Result<Vec<HoldFact>, sqlx::Error> {
        let rows: Vec<(i64, String, String, String)> = sqlx::query_as(
            "SELECT held_at, source, rules_version, CAST(unread_mids AS TEXT) FROM hold_facts
              WHERE account_address = $1 AND thread_id = $2
              ORDER BY id",
        )
        .bind(user)
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(at, source, rules_version, mids)| {
                Ok(HoldFact {
                    at,
                    source,
                    rules_version,
                    unread_mids: serde_json::from_str(&mids)
                        .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
                })
            })
            .collect()
    }

    /// Drop the holds of a conversation deleted for good.
    pub(crate) async fn drop_hold_facts(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM hold_facts WHERE account_address = $1 AND thread_id = $2")
            .bind(user)
            .bind(thread_id)
            .execute(&self.pool)
            .await?;
        Ok(())
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

/// One hold of a conversation, as stored. Nothing in the product reads it
/// yet: it is what a later look at "what was unread when this was held" has
/// to go on, the way [`ReleaseFact`] is for releases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldFact {
    /// Unix seconds.
    pub at: i64,
    /// `ingest` or `report`.
    pub source: String,
    /// The rule pack whose verdict held it; empty for a report.
    pub rules_version: String,
    /// The reader's messages not read when it was held.
    pub unread_mids: Vec<String>,
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
