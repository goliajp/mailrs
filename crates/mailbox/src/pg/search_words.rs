//! The words conversation search matches, cut by [`crate::search`]'s
//! tokenizer, the one the kevy store indexes with, and kept as `simple`
//! text vectors: every word is already lowercase and spaced, so `simple`
//! takes them as they are and the two stores hold the same words.

use std::collections::HashSet;

use crate::pg::PgMailboxStore;
use crate::search::{Stage, indexed_words};

impl PgMailboxStore {
    /// Cut the words of message row `id` again: its subject, sender and
    /// preview, and its body. Called after anything writes one of them.
    pub async fn write_search_words(&self, id: i64) -> Result<(), sqlx::Error> {
        let row: Option<(String, String, String, String)> = sqlx::query_as(
            "SELECT COALESCE(subject, ''), COALESCE(sender, ''), snippet,
                    COALESCE(clean_text, text_body, '')
               FROM messages WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((subject, sender, snippet, body)) = row else {
            return Ok(());
        };
        let header = indexed_words(&format!("{subject} {sender} {snippet}"));
        sqlx::query(
            "UPDATE messages SET header_words = to_tsvector('simple', $2),
                                 body_words = to_tsvector('simple', $3)
              WHERE id = $1",
        )
        .bind(id)
        .bind(header)
        .bind(indexed_words(&body))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Cut the words of the attachments of the message with Message-ID
    /// `mid`, in the transaction that wrote their text.
    pub(crate) async fn write_attachment_words(
        tx: &mut sqlx::Transaction<'_, crate::pg::BackendDb>,
        mid: &str,
    ) -> Result<(), sqlx::Error> {
        let rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT ac.id, COALESCE(ac.extracted_text, '')
               FROM attachment_content ac JOIN messages m ON m.id = ac.message_id
              WHERE m.message_id = $1",
        )
        .bind(mid)
        .fetch_all(&mut **tx)
        .await?;
        for (id, text) in rows {
            sqlx::query(
                "UPDATE attachment_content SET words = to_tsvector('simple', $2) WHERE id = $1",
            )
            .bind(id)
            .bind(indexed_words(&text))
            .execute(&mut **tx)
            .await?;
        }
        Ok(())
    }

    /// Cut the words of up to `batch` messages and attachments recorded
    /// before the words were kept; how many were cut. Zero once there are
    /// none left, which is the steady state.
    pub async fn fill_search_words(&self, batch: i64) -> Result<usize, sqlx::Error> {
        let ids: Vec<(i64,)> =
            sqlx::query_as("SELECT id FROM messages WHERE header_words IS NULL LIMIT $1")
                .bind(batch)
                .fetch_all(&self.pool)
                .await?;
        for (id,) in &ids {
            self.write_search_words(*id).await?;
        }
        let attachments: Vec<(i64, String)> = sqlx::query_as(
            "SELECT id, COALESCE(extracted_text, '') FROM attachment_content
              WHERE words IS NULL LIMIT $1",
        )
        .bind(batch)
        .fetch_all(&self.pool)
        .await?;
        for (id, text) in &attachments {
            sqlx::query(
                "UPDATE attachment_content SET words = to_tsvector('simple', $2) WHERE id = $1",
            )
            .bind(id)
            .bind(indexed_words(text))
            .execute(&self.pool)
            .await?;
        }
        Ok(ids.len() + attachments.len())
    }

    /// `user`'s conversations `word` is a word of, at the best stage it is
    /// found at, at most `want` of each stage, newest first within one.
    pub async fn word_threads(
        &self,
        user: &str,
        word: &str,
        want: i64,
    ) -> Result<Vec<(String, Stage)>, sqlx::Error> {
        let stages = [
            (
                Stage::Header,
                "m.header_words @@ plainto_tsquery('simple', $2)",
            ),
            (Stage::Body, "m.body_words @@ plainto_tsquery('simple', $2)"),
            (
                Stage::Attachment,
                "m.id IN (SELECT message_id FROM attachment_content
                           WHERE words @@ plainto_tsquery('simple', $2))",
            ),
        ];
        let mut out = Vec::new();
        for (stage, matched) in stages {
            let sql = format!(
                "SELECT m.thread_id FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
                  WHERE mb.user_address = $1 AND m.thread_id != '' AND {matched}
                  GROUP BY m.thread_id ORDER BY MAX(m.internal_date) DESC, m.thread_id
                  LIMIT $3"
            );
            let rows: Vec<(String,)> = sqlx::query_as(&sql)
                .bind(user)
                .bind(word)
                .bind(want)
                .fetch_all(&self.pool)
                .await?;
            out.extend(rows.into_iter().map(|(t,)| (t, stage)));
        }
        Ok(out)
    }

    /// `user`'s conversations whose subject, sender or preview contains
    /// `needle`, newest first, at most `want`, leaving out those in `skip`.
    /// One branch per column, so each can use its own trigram index.
    pub async fn substring_threads(
        &self,
        user: &str,
        needle: &str,
        skip: &HashSet<String>,
        want: usize,
    ) -> Result<Vec<String>, sqlx::Error> {
        let pattern = format!(
            "%{}%",
            needle
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let rows: Vec<(String,)> = sqlx::query_as(
            "WITH matched AS (
               SELECT id FROM messages WHERE subject IS NOT NULL AND subject != '' AND subject ILIKE $2
               UNION SELECT id FROM messages WHERE sender IS NOT NULL AND sender != '' AND sender ILIKE $2
               UNION SELECT id FROM messages WHERE snippet != '' AND snippet ILIKE $2
             )
             SELECT m.thread_id FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
              WHERE mb.user_address = $1 AND m.thread_id != ''
                AND m.id IN (SELECT id FROM matched)
              GROUP BY m.thread_id ORDER BY MAX(m.internal_date) DESC, m.thread_id
              LIMIT $3",
        )
        .bind(user)
        .bind(pattern)
        .bind((want + skip.len()) as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(t,)| t)
            .filter(|t| !skip.contains(t))
            .take(want)
            .collect())
    }
}
