//! Template fingerprints for reported scams (`message_templates`), and the
//! lookups a report makes: who has a message, in which conversation, and
//! whether it is held.

use crate::pg::PgMailboxStore;

/// How long a message stays findable by its fingerprint, in days.
pub const TEMPLATE_DAYS: i64 = 30;

/// A stored fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTemplate {
    /// SimHash of the normalised body.
    pub simhash: u64,
    /// When the message arrived, Unix seconds.
    pub arrived: i64,
    /// Registrable domains its links point to.
    pub link_domains: Vec<String>,
    /// SHA-256 of each attachment, lowercase hex.
    pub attachments: Vec<String>,
    /// Registrable domain of the `From:` address.
    pub sender_domain: String,
}

fn piece(h: u64, i: u32) -> i32 {
    i32::from((h >> (16 * i)) as u16)
}

impl PgMailboxStore {
    /// Records `message_id`'s fingerprint once; later copies leave it. The
    /// four pieces are written only when it arrived inside the window.
    pub async fn record_template(
        &self,
        message_id: &str,
        t: &StoredTemplate,
        now: i64,
    ) -> Result<bool, sqlx::Error> {
        let findable = t.arrived > now - TEMPLATE_DAYS * 86_400;
        let p = |i: u32| findable.then(|| piece(t.simhash, i));
        let done = sqlx::query(
            "INSERT INTO message_templates
                 (message_id, simhash, arrived, links, files, sender, s0, s1, s2, s3)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (message_id) DO NOTHING",
        )
        .bind(message_id)
        .bind(t.simhash as i64)
        .bind(t.arrived)
        .bind(t.link_domains.join(" "))
        .bind(t.attachments.join(" "))
        .bind(&t.sender_domain)
        .bind(p(0))
        .bind(p(1))
        .bind(p(2))
        .bind(p(3))
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() > 0)
    }

    /// The fingerprint recorded for `message_id`, if any.
    pub async fn template_of(
        &self,
        message_id: &str,
    ) -> Result<Option<StoredTemplate>, sqlx::Error> {
        let row: Option<(i64, i64, String, String, String)> = sqlx::query_as(
            "SELECT simhash, arrived, links, files, sender
               FROM message_templates WHERE message_id = $1",
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;
        let words = |s: String| s.split_whitespace().map(str::to_string).collect();
        Ok(
            row.map(|(h, arrived, links, files, sender)| StoredTemplate {
                simhash: h as u64,
                arrived,
                link_domains: words(links),
                attachments: words(files),
                sender_domain: sender,
            }),
        )
    }

    /// Messages arrived at `since` or later sharing a piece with `simhash`.
    /// Pieces of messages past the window are cleared first, so what is
    /// indexed is only the window.
    pub async fn template_candidates(
        &self,
        simhash: u64,
        since: i64,
        limit: i64,
    ) -> Result<Vec<String>, sqlx::Error> {
        let cutoff = chrono::Utc::now().timestamp() - TEMPLATE_DAYS * 86_400;
        sqlx::query(
            "UPDATE message_templates SET s0 = NULL, s1 = NULL, s2 = NULL, s3 = NULL
              WHERE arrived < $1 AND s0 IS NOT NULL",
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await?;
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT message_id FROM message_templates
              WHERE (s0 = $1 OR s1 = $2 OR s2 = $3 OR s3 = $4) AND arrived >= $5
              ORDER BY message_id LIMIT $6",
        )
        .bind(piece(simhash, 0))
        .bind(piece(simhash, 1))
        .bind(piece(simhash, 2))
        .bind(piece(simhash, 3))
        .bind(since)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(m,)| m).collect())
    }

    /// The accounts holding a copy of `message_id`.
    pub async fn message_readers(&self, message_id: &str) -> Result<Vec<String>, sqlx::Error> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT mb.user_address
               FROM messages m JOIN mailboxes mb ON mb.id = m.mailbox_id
              WHERE m.message_id = $1
              ORDER BY mb.user_address",
        )
        .bind(message_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(u,)| u).collect())
    }

    /// The conversation `user`'s copy of `message_id` is in, the file it is
    /// stored as, and whether the conversation is held.
    pub async fn message_conversation(
        &self,
        user: &str,
        message_id: &str,
    ) -> Result<Option<(String, String, bool)>, sqlx::Error> {
        sqlx::query_as(
            "SELECT m.thread_id, m.maildir_id,
                    EXISTS (SELECT 1 FROM quarantine_holds q
                             WHERE q.account_address = $1 AND q.thread_id = m.thread_id)
               FROM messages m JOIN mailboxes mb ON mb.id = m.mailbox_id
              WHERE mb.user_address = $1 AND m.message_id = $2
              ORDER BY m.id LIMIT 1",
        )
        .bind(user)
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
    }

    /// Hold `thread_id` for `user` unless it already is; true when this
    /// call held it.
    pub async fn hold_thread_if_free(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<bool, sqlx::Error> {
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

    /// Fingerprints stored, with how many are still findable.
    pub async fn template_footprint(&self) -> Result<(i64, i64), sqlx::Error> {
        sqlx::query_as("SELECT COUNT(*), COUNT(s0) FROM message_templates")
            .fetch_one(&self.pool)
            .await
    }
}
