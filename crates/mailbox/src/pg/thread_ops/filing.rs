//! Filing a conversation into a triage bucket, and marking many as read.

use crate::pg::{Held, PgMailboxStore, ThreadQuery};
use crate::scope::Bucket;

impl PgMailboxStore {
    /// File `thread_id` into `bucket` for `user`. Filing is a decision about
    /// where the conversation lives now, so an archived one comes back out,
    /// as it does in the kevy store. Returns how many messages were filed.
    pub async fn file_thread(
        &self,
        user: &str,
        thread_id: &str,
        bucket: Bucket,
    ) -> Result<u32, sqlx::Error> {
        let filed = self
            .set_thread_bucket(user, thread_id, bucket.category())
            .await?;
        if filed > 0 {
            self.unarchive_thread(user, thread_id).await?;
        }
        Ok(filed)
    }

    /// Mark every conversation `q` selects as read, and say how many had
    /// unread mail. A view's "mark all as read" passes the view's own
    /// query, so it marks the rows on screen and no others.
    pub async fn mark_rows_read(&self, q: &ThreadQuery<'_>) -> Result<u32, sqlx::Error> {
        let unread = ThreadQuery {
            unread: true,
            ..q.clone()
        };
        let mut flipped = 0;
        for row in self.thread_rows(&unread).await? {
            self.mark_thread_read(q.user, &row.thread_id, None).await?;
            flipped += 1;
        }
        Ok(flipped)
    }

    /// Mark everything `user` has unread as read: live and archived, held
    /// or not, Junk included. Returns how many conversations changed.
    pub async fn mark_all_read(&self, user: &str) -> Result<u32, sqlx::Error> {
        let q = ThreadQuery {
            archived: None,
            held: Held::Include,
            ..ThreadQuery::all(user)
        };
        self.mark_rows_read(&q).await
    }

    /// What `user` sent, newest first: `(uid, message_id, thread_id,
    /// recipients, subject, internal_date)`, deleted messages left out.
    pub async fn sent_messages(
        &self,
        user: &str,
    ) -> Result<Vec<(i32, String, String, String, String, i64)>, sqlx::Error> {
        sqlx::query_as(
            "SELECT m.uid, m.message_id, m.thread_id, m.recipients, m.subject, m.internal_date
               FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
              WHERE mb.user_address = $1 AND mb.name = 'Sent' AND (m.flags & 8) = 0
              ORDER BY m.internal_date DESC, m.id DESC",
        )
        .bind(user)
        .fetch_all(&self.pool)
        .await
    }

    /// The raw bytes of every message `user` has in `thread_id`, read from
    /// the maildir under `maildir_root`. A mailbox other than INBOX is the
    /// Maildir++ subfolder of its name. A file that is gone is left out.
    pub async fn thread_raw_messages(
        &self,
        user: &str,
        thread_id: &str,
        maildir_root: &str,
    ) -> Result<Vec<Vec<u8>>, sqlx::Error> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT m.maildir_id, mb.name FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
              WHERE m.thread_id = $1 AND mb.user_address = $2 AND m.maildir_id != ''",
        )
        .bind(thread_id)
        .bind(user)
        .fetch_all(&self.pool)
        .await?;
        let Some((local, domain)) = user.split_once('@') else {
            return Ok(Vec::new());
        };
        let base = std::path::Path::new(maildir_root).join(domain).join(local);
        let mut out = Vec::with_capacity(rows.len());
        for (id, mailbox) in rows {
            let dir = match mailbox.eq_ignore_ascii_case("INBOX") {
                true => base.clone(),
                false => base.join(format!(".{mailbox}")),
            };
            let md = mailrs_maildir::Maildir::open(&dir);
            let entries = md
                .scan_cur()
                .unwrap_or_default()
                .into_iter()
                .chain(md.scan_new().unwrap_or_default());
            if let Some(path) = entries
                .into_iter()
                .find(|e| e.id.to_string() == id)
                .map(|e| e.path)
            {
                out.push(std::fs::read(path).map_err(sqlx::Error::Io)?);
            }
        }
        Ok(out)
    }
}
