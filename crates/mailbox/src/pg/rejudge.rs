//! What re-judging reads and writes in SQL beyond the ledger: a reader's
//! conversation as it weighs it, the verdict it replaces, and its own
//! records, kept under the kevy store's key names.

use crate::pg::PgMailboxStore;

impl PgMailboxStore {
    /// `user`'s conversation `thread_id`: whether it is held and whether it
    /// is in Junk; `None` when they have no message in it.
    pub async fn rejudge_conversation(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Option<(bool, bool)>, sqlx::Error> {
        let (n, junk): (i64, Option<bool>) = sqlx::query_as(
            "SELECT COUNT(*), BOOL_OR(mb.name = 'Junk')
               FROM messages m JOIN mailboxes mb ON mb.id = m.mailbox_id
              WHERE mb.user_address = $1 AND m.thread_id = $2",
        )
        .bind(user)
        .bind(thread_id)
        .fetch_one(&self.pool)
        .await?;
        if n == 0 {
            return Ok(None);
        }
        Ok(Some((
            self.is_held(user, thread_id).await?,
            junk.unwrap_or(false),
        )))
    }

    /// The Message-IDs of `user`'s conversation `thread_id`.
    pub async fn conversation_message_ids(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT m.message_id
               FROM messages m JOIN mailboxes mb ON mb.id = m.mailbox_id
              WHERE mb.user_address = $1 AND m.thread_id = $2
              ORDER BY m.message_id",
        )
        .bind(user)
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(m,)| m).collect())
    }

    /// Replace the verdict stored for `message_id`: a re-judge's verdict
    /// supersedes the one given on arrival.
    pub async fn replace_fraud_verdict(
        &self,
        message_id: &str,
        verdict_json: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO fraud_verdicts (message_id, verdict) VALUES ($1, CAST($2 AS JSONB))
             ON CONFLICT (message_id) DO UPDATE SET verdict = EXCLUDED.verdict",
        )
        .bind(message_id)
        .bind(verdict_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Field `field` of re-judge record `key` (`""` for a plain value).
    pub async fn rejudge_get(&self, key: &str, field: &str) -> Result<Option<String>, sqlx::Error> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM rejudge_kv WHERE key = $1 AND field = $2")
                .bind(key)
                .bind(field)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(v,)| v))
    }

    /// Set field `field` of re-judge record `key`.
    pub async fn rejudge_put(
        &self,
        key: &str,
        field: &str,
        value: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO rejudge_kv (key, field, value) VALUES ($1, $2, $3)
             ON CONFLICT (key, field) DO UPDATE SET value = EXCLUDED.value",
        )
        .bind(key)
        .bind(field)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Remove field `field` of re-judge record `key`.
    pub async fn rejudge_delete_field(&self, key: &str, field: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM rejudge_kv WHERE key = $1 AND field = $2")
            .bind(key)
            .bind(field)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Every field of re-judge record `key`.
    pub async fn rejudge_fields(&self, key: &str) -> Result<Vec<(String, String)>, sqlx::Error> {
        sqlx::query_as("SELECT field, value FROM rejudge_kv WHERE key = $1 ORDER BY field")
            .bind(key)
            .fetch_all(&self.pool)
            .await
    }

    /// Remove record `key`, whatever it holds.
    pub async fn rejudge_delete(&self, key: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM rejudge_kv WHERE key = $1")
            .bind(key)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM rejudge_sets WHERE key = $1")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Add `member` to re-judge set `key`.
    pub async fn rejudge_add_member(&self, key: &str, member: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO rejudge_sets (key, member) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(key)
        .bind(member)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Remove `member` from re-judge set `key`.
    pub async fn rejudge_remove_member(&self, key: &str, member: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM rejudge_sets WHERE key = $1 AND member = $2")
            .bind(key)
            .bind(member)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The members of re-judge set `key`.
    pub async fn rejudge_members(&self, key: &str) -> Result<Vec<String>, sqlx::Error> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT member FROM rejudge_sets WHERE key = $1 ORDER BY member")
                .bind(key)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|(m,)| m).collect())
    }

    /// Every re-judge record whose key starts with `prefix`. The records are
    /// few (a handful per observation and per conversation it moved), so
    /// they are listed whole and matched here.
    pub async fn rejudge_keys(&self, prefix: &str) -> Result<Vec<String>, sqlx::Error> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT key FROM rejudge_kv UNION SELECT DISTINCT key FROM rejudge_sets",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(k,)| k)
            .filter(|k| k.starts_with(prefix))
            .collect())
    }
}
