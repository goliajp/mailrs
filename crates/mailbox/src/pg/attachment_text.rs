//! The queue of copies whose attachments are read, and what reading them
//! came to. A message is named by its `Message-ID`; what is stored is kept
//! on every copy of it (`attachment_content` for the text,
//! `attachment_text_computed` for the outcome), so it goes with the last
//! copy.

use crate::attachment_text::AttachmentTextState;
use crate::pg::PgMailboxStore;

const STATE_COLUMNS: &str =
    "version, parts, pdfs, images, stored, ocr, failed, too_large, too_small, truncated";

type StateRow = (i32, i32, i32, i32, i32, i32, i32, i32, i32, i32);

fn state_of(r: StateRow) -> AttachmentTextState {
    let n = |v: i32| u32::try_from(v).unwrap_or(0);
    AttachmentTextState {
        version: n(r.0),
        parts: n(r.1),
        pdfs: n(r.2),
        images: n(r.3),
        stored: n(r.4),
        ocr: n(r.5),
        failed: n(r.6),
        too_large: n(r.7),
        too_small: n(r.8),
        truncated: n(r.9),
    }
}

fn int(v: u32) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}

impl PgMailboxStore {
    /// Queue `user`'s copy of the message for reading.
    pub async fn queue_attachment_text(&self, user: &str, mid: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO attachment_text_queue (user_address, message_id) VALUES ($1, $2)
             ON CONFLICT (user_address, message_id) DO NOTHING",
        )
        .bind(user)
        .bind(mid)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Up to `limit` queued `(user, message id)` pairs, left in the queue.
    pub async fn pending_attachment_texts(
        &self,
        limit: usize,
    ) -> Result<Vec<(String, String)>, sqlx::Error> {
        sqlx::query_as("SELECT user_address, message_id FROM attachment_text_queue LIMIT $1")
            .bind(i64::try_from(limit).unwrap_or(i64::MAX))
            .fetch_all(&self.pool)
            .await
    }

    /// Count one more attempt at a queued copy, committed before it
    /// returns. Returns the attempts so far, this one included.
    pub async fn begin_attachment_text(&self, user: &str, mid: &str) -> Result<u32, sqlx::Error> {
        let (n,): (i32,) = sqlx::query_as(
            "UPDATE attachment_text_queue SET attempts = attempts + 1
             WHERE user_address = $1 AND message_id = $2 RETURNING attempts",
        )
        .bind(user)
        .bind(mid)
        .fetch_one(&self.pool)
        .await?;
        Ok(u32::try_from(n).unwrap_or(0))
    }

    /// Take the copy off the queue, its attempts with it.
    pub async fn dequeue_attachment_text(&self, user: &str, mid: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "DELETE FROM attachment_text_queue WHERE user_address = $1 AND message_id = $2",
        )
        .bind(user)
        .bind(mid)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Give up on the copy: off the queue, into the set-aside table.
    pub async fn set_aside_attachment_text(
        &self,
        user: &str,
        mid: &str,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO attachment_text_set_aside (user_address, message_id) VALUES ($1, $2)
             ON CONFLICT (user_address, message_id) DO NOTHING",
        )
        .bind(user)
        .bind(mid)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM attachment_text_queue WHERE user_address = $1 AND message_id = $2",
        )
        .bind(user)
        .bind(mid)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }

    /// Whether the copy is queued.
    pub async fn attachment_text_queued(&self, user: &str, mid: &str) -> Result<bool, sqlx::Error> {
        let row: Option<(i32,)> = sqlx::query_as(
            "SELECT attempts FROM attachment_text_queue WHERE user_address = $1 AND message_id = $2",
        )
        .bind(user)
        .bind(mid)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    /// How many copies were given up on.
    pub async fn attachment_texts_set_aside(&self) -> Result<usize, sqlx::Error> {
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM attachment_text_set_aside")
            .fetch_one(&self.pool)
            .await?;
        Ok(usize::try_from(n).unwrap_or(0))
    }

    /// The maildir file name of `user`'s copy of the message, its first if
    /// there are several.
    pub async fn copy_file_of(&self, user: &str, mid: &str) -> Result<Option<String>, sqlx::Error> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT m.maildir_id FROM messages m JOIN mailboxes mb ON mb.id = m.mailbox_id
             WHERE m.message_id = $1 AND mb.user_address = $2 ORDER BY m.id LIMIT 1",
        )
        .bind(mid)
        .bind(user)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(f,)| f))
    }

    /// The copy whose outcome is read for the message: the first that has one.
    async fn read_copy(&self, mid: &str) -> Result<Option<i64>, sqlx::Error> {
        let row: Option<(i64,)> = sqlx::query_as(
            "SELECT s.message_id FROM attachment_text_computed s
             JOIN messages m ON m.id = s.message_id
             WHERE m.message_id = $1 ORDER BY s.message_id LIMIT 1",
        )
        .bind(mid)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id,)| id))
    }

    /// The outcome stored for the message, or `None` if it was never read.
    pub async fn attachment_text_state(
        &self,
        mid: &str,
    ) -> Result<Option<AttachmentTextState>, sqlx::Error> {
        let Some(copy) = self.read_copy(mid).await? else {
            return Ok(None);
        };
        let row: Option<StateRow> = sqlx::query_as(&format!(
            "SELECT {STATE_COLUMNS} FROM attachment_text_computed WHERE message_id = $1"
        ))
        .bind(copy)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(state_of))
    }

    /// The stored text of each attachment of the message, in attachment
    /// order.
    pub async fn attachment_texts(&self, mid: &str) -> Result<Vec<String>, sqlx::Error> {
        let Some(copy) = self.read_copy(mid).await? else {
            return Ok(Vec::new());
        };
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT COALESCE(extracted_text, '') FROM attachment_content
             WHERE message_id = $1 AND attachment_index >= 0 AND extracted_text != ''
             ORDER BY attachment_index",
        )
        .bind(copy)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(t,)| t).collect())
    }

    /// The stored QR addresses of the message, one per line, `''` when none.
    pub async fn attachment_qr_lines(&self, mid: &str) -> Result<String, sqlx::Error> {
        let Some(copy) = self.read_copy(mid).await? else {
            return Ok(String::new());
        };
        let (lines,): (String,) =
            sqlx::query_as("SELECT qr_urls FROM attachment_text_computed WHERE message_id = $1")
                .bind(copy)
                .fetch_one(&self.pool)
                .await?;
        Ok(lines)
    }

    /// Store what reading the message came to on every copy of it,
    /// replacing what was there: `texts` are `(position, text)`, already
    /// trimmed, cut and without empty ones. Its QR addresses start empty.
    pub async fn store_attachment_texts(
        &self,
        mid: &str,
        texts: &[(usize, String)],
        state: &AttachmentTextState,
    ) -> Result<(), sqlx::Error> {
        let copies = "SELECT id FROM messages WHERE message_id = $1";
        let mut tx = self.pool.begin().await?;
        for table in ["attachment_content", "attachment_text_computed"] {
            sqlx::query(&format!(
                "DELETE FROM {table} WHERE message_id IN ({copies})"
            ))
            .bind(mid)
            .execute(&mut *tx)
            .await?;
        }
        for (n, text) in texts {
            sqlx::query(
                "INSERT INTO attachment_content (message_id, attachment_index, content_type, extracted_text)
                 SELECT id, $2::SMALLINT, '', $3::TEXT FROM messages WHERE message_id = $1",
            )
            .bind(mid)
            .bind(i16::try_from(*n).unwrap_or(i16::MAX))
            .bind(text)
            .execute(&mut *tx)
            .await?;
        }
        let sql = format!(
            "INSERT INTO attachment_text_computed (message_id, {STATE_COLUMNS})
             SELECT id, $2::INTEGER, $3::INTEGER, $4::INTEGER, $5::INTEGER, $6::INTEGER,
                    $7::INTEGER, $8::INTEGER, $9::INTEGER, $10::INTEGER, $11::INTEGER
             FROM messages WHERE message_id = $1"
        );
        let mut insert = sqlx::query(&sql).bind(mid);
        for (_, v) in state.fields() {
            insert = insert.bind(int(v));
        }
        insert.execute(&mut *tx).await?;
        tx.commit().await
    }

    /// Set the QR addresses (one per line) of every copy that has an
    /// outcome, and its version when `version` is given.
    pub async fn store_attachment_qr(
        &self,
        mid: &str,
        lines: &str,
        version: Option<u32>,
    ) -> Result<(), sqlx::Error> {
        let set_version = match version {
            Some(_) => ", version = $3::INTEGER",
            None => "",
        };
        let sql = format!(
            "UPDATE attachment_text_computed SET qr_urls = $2::TEXT{set_version}
             WHERE message_id IN (SELECT id FROM messages WHERE message_id = $1)"
        );
        let mut q = sqlx::query(&sql).bind(mid).bind(lines);
        if let Some(v) = version {
            q = q.bind(int(v));
        }
        q.execute(&self.pool).await?;
        Ok(())
    }

    /// Copy the outcome stored for the message, and its text, onto those
    /// of `user`'s copies that have none: a copy delivered after the
    /// message was read.
    pub async fn share_attachment_text(&self, user: &str, mid: &str) -> Result<(), sqlx::Error> {
        let Some(from) = self.read_copy(mid).await? else {
            return Ok(());
        };
        let mine: Vec<(i64,)> = sqlx::query_as(
            "SELECT m.id FROM messages m JOIN mailboxes mb ON mb.id = m.mailbox_id
             WHERE m.message_id = $1 AND mb.user_address = $2",
        )
        .bind(mid)
        .bind(user)
        .fetch_all(&self.pool)
        .await?;
        let read: Vec<(i64,)> = sqlx::query_as(
            "SELECT s.message_id FROM attachment_text_computed s
             JOIN messages m ON m.id = s.message_id WHERE m.message_id = $1",
        )
        .bind(mid)
        .fetch_all(&self.pool)
        .await?;
        let mut tx = self.pool.begin().await?;
        for (to,) in mine.into_iter().filter(|c| !read.contains(c)) {
            sqlx::query("DELETE FROM attachment_content WHERE message_id = $1")
                .bind(to)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "INSERT INTO attachment_content (message_id, attachment_index, content_type, extracted_text)
                 SELECT $1::BIGINT, attachment_index, content_type, extracted_text
                 FROM attachment_content WHERE message_id = $2 AND attachment_index >= 0",
            )
            .bind(to)
            .bind(from)
            .execute(&mut *tx)
            .await?;
            sqlx::query(&format!(
                "INSERT INTO attachment_text_computed (message_id, {STATE_COLUMNS}, qr_urls)
                 SELECT $1::BIGINT, {STATE_COLUMNS}, qr_urls
                 FROM attachment_text_computed WHERE message_id = $2"
            ))
            .bind(to)
            .bind(from)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }
}
