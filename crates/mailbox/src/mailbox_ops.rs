use sqlx::PgPool;

use crate::store::MailboxStore;
use crate::types::Mailbox;

impl MailboxStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// create a mailbox, returns it if already exists
    pub async fn create_mailbox(&self, user: &str, name: &str) -> Result<Mailbox, sqlx::Error> {
        let uidvalidity = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i32;

        sqlx::query(
            "INSERT INTO mailboxes (user_address, name, uidvalidity) VALUES ($1, $2, $3)
             ON CONFLICT DO NOTHING",
        )
        .bind(user)
        .bind(name)
        .bind(uidvalidity)
        .execute(&self.pool)
        .await?;

        let row = sqlx::query_as::<_, (i64, String, String, i32, i32, i64)>(
            "SELECT id, user_address, name, uidvalidity, uidnext, highest_modseq
             FROM mailboxes WHERE user_address = $1 AND name = $2",
        )
        .bind(user)
        .bind(name)
        .fetch_one(&self.pool)
        .await?;

        Ok(Mailbox {
            id: row.0,
            user: row.1,
            name: row.2,
            uidvalidity: row.3 as u32,
            uidnext: row.4 as u32,
            highest_modseq: row.5 as u64,
        })
    }

    pub async fn get_mailbox(
        &self,
        user: &str,
        name: &str,
    ) -> Result<Option<Mailbox>, sqlx::Error> {
        let row = sqlx::query_as::<_, (i64, String, String, i32, i32, i64)>(
            "SELECT id, user_address, name, uidvalidity, uidnext, highest_modseq
             FROM mailboxes WHERE user_address = $1 AND name = $2",
        )
        .bind(user)
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| Mailbox {
            id: r.0,
            user: r.1,
            name: r.2,
            uidvalidity: r.3 as u32,
            uidnext: r.4 as u32,
            highest_modseq: r.5 as u64,
        }))
    }

    pub async fn get_mailbox_by_id(&self, id: i64) -> Result<Option<Mailbox>, sqlx::Error> {
        let row = sqlx::query_as::<_, (i64, String, String, i32, i32, i64)>(
            "SELECT id, user_address, name, uidvalidity, uidnext, highest_modseq
             FROM mailboxes WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| Mailbox {
            id: r.0,
            user: r.1,
            name: r.2,
            uidvalidity: r.3 as u32,
            uidnext: r.4 as u32,
            highest_modseq: r.5 as u64,
        }))
    }

    pub async fn list_mailboxes(&self, user: &str) -> Result<Vec<Mailbox>, sqlx::Error> {
        let rows = sqlx::query_as::<_, (i64, String, String, i32, i32, i64)>(
            "SELECT id, user_address, name, uidvalidity, uidnext, highest_modseq
             FROM mailboxes WHERE user_address = $1 ORDER BY name",
        )
        .bind(user)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| Mailbox {
                id: r.0,
                user: r.1,
                name: r.2,
                uidvalidity: r.3 as u32,
                uidnext: r.4 as u32,
                highest_modseq: r.5 as u64,
            })
            .collect())
    }

    pub async fn delete_mailbox(&self, user: &str, name: &str) -> Result<bool, sqlx::Error> {
        // messages are CASCADE-deleted via FK, but be explicit
        sqlx::query(
            "DELETE FROM messages WHERE mailbox_id IN
             (SELECT id FROM mailboxes WHERE user_address = $1 AND name = $2)",
        )
        .bind(user)
        .bind(name)
        .execute(&self.pool)
        .await?;

        let result = sqlx::query("DELETE FROM mailboxes WHERE user_address = $1 AND name = $2")
            .bind(user)
            .bind(name)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn rename_mailbox(
        &self,
        user: &str,
        old_name: &str,
        new_name: &str,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE mailboxes SET name = $3 WHERE user_address = $1 AND name = $2",
        )
        .bind(user)
        .bind(old_name)
        .bind(new_name)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// create default mailboxes (INBOX, Sent, Drafts, Trash, Junk) if they don't exist
    pub async fn ensure_default_mailboxes(&self, user: &str) -> Result<(), sqlx::Error> {
        for name in &["INBOX", "Sent", "Drafts", "Trash", "Junk"] {
            self.create_mailbox(user, name).await?;
        }
        Ok(())
    }

    /// count (total, unseen) messages in a mailbox
    /// unseen excludes spam/scam to stay consistent with conversation view
    pub async fn mailbox_status(&self, mailbox_id: i64) -> Result<(u32, u32), sqlx::Error> {
        let total: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM messages WHERE mailbox_id = $1")
                .bind(mailbox_id)
                .fetch_one(&self.pool)
                .await?;
        let unseen: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM messages WHERE mailbox_id = $1 AND (flags & 1) = 0 \
             AND NOT EXISTS (SELECT 1 FROM email_analysis ea WHERE ea.message_id = messages.id AND ea.category IN ('spam', 'scam'))",
        )
        .bind(mailbox_id)
        .fetch_one(&self.pool)
        .await?;
        Ok((total.0 as u32, unseen.0 as u32))
    }
}
