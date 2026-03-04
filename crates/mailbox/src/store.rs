use sqlx::PgPool;

use crate::threading;
use crate::types::{ConversationSummary, FlagAction, Mailbox, MessageMeta, FLAG_DELETED, FLAG_SEEN};

/// mailbox storage backed by PG for metadata and maildir for message bodies
pub struct MailboxStore {
    pool: PgPool,
}

impl MailboxStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// create a mailbox, returns it if already exists
    pub async fn create_mailbox(&self, user: &str, name: &str) -> Result<Mailbox, sqlx::Error> {
        let uidvalidity = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
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

    /// create default mailboxes (INBOX, Sent, Drafts, Trash, Junk) if they don't exist
    pub async fn ensure_default_mailboxes(&self, user: &str) -> Result<(), sqlx::Error> {
        for name in &["INBOX", "Sent", "Drafts", "Trash", "Junk"] {
            self.create_mailbox(user, name).await?;
        }
        Ok(())
    }

    /// index a new message: assigns UID, inserts metadata, returns UID
    pub async fn index_message(
        &self,
        user: &str,
        mailbox_name: &str,
        maildir_id: &str,
        sender: &str,
        recipients: &str,
        subject: &str,
        size: u32,
        now: i64,
        message_id: &str,
        in_reply_to: &str,
        thread_id: &str,
    ) -> Result<u32, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        // lock mailbox row to prevent concurrent UID allocation
        let (mailbox_id, uidnext, highest_modseq) =
            sqlx::query_as::<_, (i64, i32, i64)>(
                "SELECT id, uidnext, highest_modseq FROM mailboxes
                 WHERE user_address = $1 AND name = $2 FOR UPDATE",
            )
            .bind(user)
            .bind(mailbox_name)
            .fetch_one(&mut *tx)
            .await?;

        let uid = uidnext;
        let new_modseq = highest_modseq + 1;

        // insert message
        sqlx::query(
            "INSERT INTO messages (mailbox_id, uid, maildir_id, sender, recipients, subject, size, date_epoch, internal_date, message_id, in_reply_to, thread_id, modseq)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(mailbox_id)
        .bind(uid)
        .bind(maildir_id)
        .bind(sender)
        .bind(recipients)
        .bind(subject)
        .bind(size as i32)
        .bind(now) // date_epoch
        .bind(now) // internal_date
        .bind(message_id)
        .bind(in_reply_to)
        .bind(thread_id)
        .bind(new_modseq)
        .execute(&mut *tx)
        .await?;

        // increment uidnext and highest_modseq
        sqlx::query("UPDATE mailboxes SET uidnext = $1, highest_modseq = $2 WHERE id = $3")
            .bind(uid + 1)
            .bind(new_modseq)
            .bind(mailbox_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;

        Ok(uid as u32)
    }

    pub async fn list_messages(
        &self,
        mailbox_id: i64,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<MessageMeta>, sqlx::Error> {
        let rows = sqlx::query_as::<_, (i64, i64, i32, String, String, String, String, i64, i32, i32, i64, String, String, String, i64)>(
            "SELECT id, mailbox_id, uid, maildir_id, sender, recipients, subject, date_epoch, size, flags, internal_date, message_id, in_reply_to, thread_id, modseq
             FROM messages WHERE mailbox_id = $1 ORDER BY uid LIMIT $2 OFFSET $3",
        )
        .bind(mailbox_id)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(row_to_message_meta).collect())
    }

    pub async fn get_message(
        &self,
        mailbox_id: i64,
        uid: u32,
    ) -> Result<Option<MessageMeta>, sqlx::Error> {
        let row = sqlx::query_as::<_, (i64, i64, i32, String, String, String, String, i64, i32, i32, i64, String, String, String, i64)>(
            "SELECT id, mailbox_id, uid, maildir_id, sender, recipients, subject, date_epoch, size, flags, internal_date, message_id, in_reply_to, thread_id, modseq
             FROM messages WHERE mailbox_id = $1 AND uid = $2",
        )
        .bind(mailbox_id)
        .bind(uid as i32)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(row_to_message_meta))
    }

    pub async fn update_flags(
        &self,
        mailbox_id: i64,
        uid: u32,
        flags: u32,
    ) -> Result<u64, sqlx::Error> {
        let modseq = bump_modseq(&self.pool, mailbox_id).await?;
        sqlx::query(
            "UPDATE messages SET flags = $1, modseq = $4 WHERE mailbox_id = $2 AND uid = $3",
        )
        .bind(flags as i32)
        .bind(mailbox_id)
        .bind(uid as i32)
        .bind(modseq as i64)
        .execute(&self.pool)
        .await?;
        Ok(modseq)
    }

    pub async fn add_flags(
        &self,
        mailbox_id: i64,
        uid: u32,
        flags: u32,
    ) -> Result<u64, sqlx::Error> {
        let modseq = bump_modseq(&self.pool, mailbox_id).await?;
        sqlx::query(
            "UPDATE messages SET flags = flags | $1, modseq = $4 WHERE mailbox_id = $2 AND uid = $3",
        )
        .bind(flags as i32)
        .bind(mailbox_id)
        .bind(uid as i32)
        .bind(modseq as i64)
        .execute(&self.pool)
        .await?;
        Ok(modseq)
    }

    pub async fn remove_flags(
        &self,
        mailbox_id: i64,
        uid: u32,
        flags: u32,
    ) -> Result<u64, sqlx::Error> {
        let modseq = bump_modseq(&self.pool, mailbox_id).await?;
        sqlx::query(
            "UPDATE messages SET flags = flags & ~$1, modseq = $4 WHERE mailbox_id = $2 AND uid = $3",
        )
        .bind(flags as i32)
        .bind(mailbox_id)
        .bind(uid as i32)
        .bind(modseq as i64)
        .execute(&self.pool)
        .await?;
        Ok(modseq)
    }

    /// conditionally update flags only if message modseq <= unchangedsince (RFC 7162 CONDSTORE)
    /// returns Ok(Some(new_modseq)) on success, Ok(None) if precondition failed
    pub async fn update_flags_if_unchanged(
        &self,
        mailbox_id: i64,
        uid: u32,
        flags: u32,
        action: FlagAction,
        unchangedsince: u64,
    ) -> Result<Option<u64>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        // check current modseq (lock row to prevent concurrent modification)
        let current: (i64,) = sqlx::query_as(
            "SELECT modseq FROM messages WHERE mailbox_id = $1 AND uid = $2 FOR UPDATE",
        )
        .bind(mailbox_id)
        .bind(uid as i32)
        .fetch_one(&mut *tx)
        .await?;

        if current.0 as u64 > unchangedsince {
            tx.rollback().await?;
            return Ok(None);
        }

        // bump modseq via RETURNING
        let row: (i64,) = sqlx::query_as(
            "UPDATE mailboxes SET highest_modseq = highest_modseq + 1
             WHERE id = $1 RETURNING highest_modseq",
        )
        .bind(mailbox_id)
        .fetch_one(&mut *tx)
        .await?;
        let modseq = row.0 as u64;

        match action {
            FlagAction::Set => {
                sqlx::query(
                    "UPDATE messages SET flags = $1, modseq = $4 WHERE mailbox_id = $2 AND uid = $3",
                )
                .bind(flags as i32)
                .bind(mailbox_id)
                .bind(uid as i32)
                .bind(modseq as i64)
                .execute(&mut *tx)
                .await?;
            }
            FlagAction::Add => {
                sqlx::query(
                    "UPDATE messages SET flags = flags | $1, modseq = $4 WHERE mailbox_id = $2 AND uid = $3",
                )
                .bind(flags as i32)
                .bind(mailbox_id)
                .bind(uid as i32)
                .bind(modseq as i64)
                .execute(&mut *tx)
                .await?;
            }
            FlagAction::Remove => {
                sqlx::query(
                    "UPDATE messages SET flags = flags & ~$1, modseq = $4 WHERE mailbox_id = $2 AND uid = $3",
                )
                .bind(flags as i32)
                .bind(mailbox_id)
                .bind(uid as i32)
                .bind(modseq as i64)
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
        Ok(Some(modseq))
    }

    /// list messages changed since a given modseq (for CONDSTORE FETCH CHANGEDSINCE)
    pub async fn list_messages_changed_since(
        &self,
        mailbox_id: i64,
        modseq: u64,
    ) -> Result<Vec<MessageMeta>, sqlx::Error> {
        let rows = sqlx::query_as::<_, (i64, i64, i32, String, String, String, String, i64, i32, i32, i64, String, String, String, i64)>(
            "SELECT id, mailbox_id, uid, maildir_id, sender, recipients, subject, date_epoch, size, flags, internal_date, message_id, in_reply_to, thread_id, modseq
             FROM messages WHERE mailbox_id = $1 AND modseq > $2 ORDER BY uid",
        )
        .bind(mailbox_id)
        .bind(modseq as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(row_to_message_meta).collect())
    }

    /// delete messages with \Deleted flag, return expunged UIDs
    pub async fn expunge(&self, mailbox_id: i64) -> Result<Vec<u32>, sqlx::Error> {
        let uid_rows = sqlx::query_as::<_, (i32,)>(
            "SELECT uid FROM messages WHERE mailbox_id = $1 AND (flags & $2) != 0",
        )
        .bind(mailbox_id)
        .bind(FLAG_DELETED as i32)
        .fetch_all(&self.pool)
        .await?;

        let uids: Vec<u32> = uid_rows.into_iter().map(|r| r.0 as u32).collect();

        sqlx::query("DELETE FROM messages WHERE mailbox_id = $1 AND (flags & $2) != 0")
            .bind(mailbox_id)
            .bind(FLAG_DELETED as i32)
            .execute(&self.pool)
            .await?;

        Ok(uids)
    }

    /// append a message: write to maildir and index it (with threading)
    /// returns (uid, maildir_id)
    pub async fn append_message(
        &self,
        user: &str,
        mailbox_name: &str,
        maildir_root: &str,
        data: &[u8],
        flags: u32,
        now: i64,
    ) -> Result<(u32, String), String> {
        // extract domain from user
        let (local, domain) = user
            .split_once('@')
            .ok_or_else(|| "invalid user address".to_string())?;

        let path = format!("{maildir_root}/{domain}/{local}");
        let md = mailrs_storage_maildir::Maildir::create(&path)
            .map_err(|e| format!("failed to create maildir: {e}"))?;

        let msg_id = md
            .deliver(data)
            .map_err(|e| format!("failed to deliver: {e}"))?;

        let sender = extract_header_value(data, "From");
        let recipients = extract_header_value(data, "To");
        let subject = extract_header_value(data, "Subject");
        let message_id = threading::extract_message_id(data);
        let in_reply_to = threading::extract_in_reply_to(data);

        let thread_id = if !message_id.is_empty() {
            let parent_tid = self
                .find_thread_id_by_message_id(user, &in_reply_to)
                .await
                .ok()
                .flatten();
            threading::resolve_thread_id(&message_id, &in_reply_to, |_| parent_tid.clone())
        } else {
            String::new()
        };

        let uid = self
            .index_message(
                user,
                mailbox_name,
                &msg_id.to_string(),
                &sender,
                &recipients,
                &subject,
                data.len() as u32,
                now,
                &message_id,
                &in_reply_to,
                &thread_id,
            )
            .await
            .map_err(|e| format!("failed to index: {e}"))?;

        // set flags if any
        if flags != 0 {
            let mb = self
                .get_mailbox(user, mailbox_name)
                .await
                .map_err(|e| format!("failed to get mailbox: {e}"))?
                .ok_or("mailbox not found")?;
            let _ = self.update_flags(mb.id, uid, flags).await;
        }

        Ok((uid, msg_id.to_string()))
    }

    /// count (total, unseen) messages in a mailbox
    pub async fn mailbox_status(&self, mailbox_id: i64) -> Result<(u32, u32), sqlx::Error> {
        let total: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM messages WHERE mailbox_id = $1")
                .bind(mailbox_id)
                .fetch_one(&self.pool)
                .await?;
        let unseen: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM messages WHERE mailbox_id = $1 AND (flags & 1) = 0",
        )
        .bind(mailbox_id)
        .fetch_one(&self.pool)
        .await?;
        Ok((total.0 as u32, unseen.0 as u32))
    }

    /// total storage used by a user across all mailboxes (in bytes)
    pub async fn user_storage_usage(&self, user: &str) -> u64 {
        let row: Result<(i64,), _> = sqlx::query_as(
            "SELECT COALESCE(SUM(m.size), 0) FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id WHERE mb.user_address = $1",
        )
        .bind(user)
        .fetch_one(&self.pool)
        .await;

        row.map(|r| r.0 as u64).unwrap_or(0)
    }

    /// find a message by its message_id header (across all user's mailboxes)
    pub async fn find_message_by_message_id(
        &self,
        user: &str,
        message_id: &str,
    ) -> Result<Option<MessageMeta>, sqlx::Error> {
        let row = sqlx::query_as::<_, (i64, i64, i32, String, String, String, String, i64, i32, i32, i64, String, String, String, i64)>(
            "SELECT m.id, m.mailbox_id, m.uid, m.maildir_id, m.sender, m.recipients,
                    m.subject, m.date_epoch, m.size, m.flags, m.internal_date, m.message_id,
                    m.in_reply_to, m.thread_id, m.modseq
             FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND m.message_id = $2
             LIMIT 1",
        )
        .bind(user)
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(row_to_message_meta))
    }

    // ---- threading / conversation queries ----

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

    /// list conversations grouped by thread_id, ordered by most recent
    pub async fn list_conversations(
        &self,
        user: &str,
        limit: u32,
        before_ts: Option<i64>,
        category: Option<&str>,
    ) -> Result<Vec<ConversationSummary>, sqlx::Error> {
        let count_expr = "COUNT(DISTINCT CASE WHEN m.message_id != '' THEN m.message_id ELSE CAST(m.id AS TEXT) END)";
        let unread_expr = "COUNT(DISTINCT CASE WHEN (m.flags & 1) = 0 THEN CASE WHEN m.message_id != '' THEN m.message_id ELSE CAST(m.id AS TEXT) END END)";

        // build dynamic WHERE clauses
        let mut conditions = vec![
            "mb.user_address = $1".to_string(),
            "thread_id != ''".to_string(),
        ];
        let mut param_idx = 3u32; // $1=user, $2=limit

        if before_ts.is_some() {
            conditions.push(format!("internal_date < ${param_idx}"));
            param_idx += 1;
        }
        if category.is_some() {
            conditions.push(format!(
                "m.id IN (SELECT ea_inner.message_id FROM email_analysis ea_inner WHERE ea_inner.category = ${param_idx})"
            ));
        }

        let where_clause = conditions.join(" AND ");
        let sql = format!(
            "SELECT m.thread_id, MAX(m.subject), string_agg(DISTINCT m.sender, ','),
                    {count_expr}, {unread_expr}, MAX(m.internal_date),
                    COALESCE((SELECT ea.category FROM email_analysis ea
                              JOIN messages m2 ON ea.message_id = m2.id
                              WHERE m2.thread_id = m.thread_id
                              ORDER BY m2.internal_date DESC LIMIT 1), 'general')
             FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE {where_clause}
             GROUP BY m.thread_id ORDER BY MAX(m.internal_date) DESC LIMIT $2"
        );

        let mut query = sqlx::query_as::<_, (String, Option<String>, Option<String>, i64, i64, i64, String)>(&sql)
            .bind(user)
            .bind(limit as i64);

        if let Some(ts) = before_ts {
            query = query.bind(ts);
        }
        if let Some(cat) = category {
            query = query.bind(cat);
        }

        let rows = query.fetch_all(&self.pool).await?;

        Ok(rows
            .into_iter()
            .map(|r| ConversationSummary {
                thread_id: r.0,
                subject: r.1.unwrap_or_default(),
                participants: r.2.unwrap_or_default(),
                message_count: r.3 as u32,
                unread_count: r.4 as u32,
                last_date: r.5,
                category: r.6,
            })
            .collect())
    }

    /// list all messages in a thread (deduplicated by message_id)
    pub async fn list_thread_messages(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Vec<MessageMeta>, sqlx::Error> {
        // deduplicate: same email may exist in both INBOX and Sent
        let rows = sqlx::query_as::<_, (i64, i64, i32, String, String, String, String, i64, i32, i32, i64, String, String, String, i64)>(
            "SELECT m.id, m.mailbox_id, m.uid, m.maildir_id, m.sender, m.recipients, m.subject,
                    m.date_epoch, m.size, m.flags, m.internal_date, m.message_id, m.in_reply_to, m.thread_id, m.modseq
             FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND m.thread_id = $2
               AND m.id = (
                 SELECT MIN(m2.id) FROM messages m2
                 JOIN mailboxes mb2 ON m2.mailbox_id = mb2.id
                 WHERE mb2.user_address = $1
                   AND CASE WHEN m.message_id != '' THEN m2.message_id = m.message_id
                            ELSE m2.id = m.id END
               )
             ORDER BY m.internal_date ASC",
        )
        .bind(user)
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(row_to_message_meta).collect())
    }

    /// get all message-ids in the thread that contains the given message_id,
    /// ordered by date (for building RFC 5322 References header)
    pub async fn get_thread_references(
        &self,
        user: &str,
        message_id: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        // first find the thread_id
        let thread_id_row = sqlx::query_as::<_, (String,)>(
            "SELECT m.thread_id FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND m.message_id = $2 AND m.thread_id != ''
             LIMIT 1",
        )
        .bind(user)
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;

        let thread_id = match thread_id_row {
            Some(r) => r.0,
            None => return Ok(vec![message_id.to_string()]),
        };

        // get all distinct message_ids in this thread, ordered by date
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT DISTINCT m.message_id
             FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND m.thread_id = $2 AND m.message_id != ''
             ORDER BY m.message_id",
        )
        .bind(user)
        .bind(&thread_id)
        .fetch_all(&self.pool)
        .await?;

        let ids: Vec<String> = rows.into_iter().map(|r| r.0).collect();
        if ids.is_empty() {
            Ok(vec![message_id.to_string()])
        } else {
            Ok(ids)
        }
    }

    /// mark all messages in a thread as read
    pub async fn mark_thread_read(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<u32, sqlx::Error> {
        // bump highest_modseq for all affected mailboxes
        sqlx::query(
            "UPDATE mailboxes SET highest_modseq = highest_modseq + 1
             WHERE id IN (
                SELECT DISTINCT mailbox_id FROM messages
                WHERE thread_id = $1 AND (flags & $2) = 0
                  AND mailbox_id IN (SELECT id FROM mailboxes WHERE user_address = $3)
             )",
        )
        .bind(thread_id)
        .bind(FLAG_SEEN as i32)
        .bind(user)
        .execute(&self.pool)
        .await?;

        // get the new modseq value (max across affected mailboxes)
        let new_modseq: (i64,) = sqlx::query_as(
            "SELECT COALESCE(MAX(highest_modseq), 0) FROM mailboxes WHERE user_address = $1",
        )
        .bind(user)
        .fetch_one(&self.pool)
        .await?;

        let result = sqlx::query(
            "UPDATE messages SET flags = flags | $1, modseq = $4
             WHERE thread_id = $2 AND (flags & $1) = 0
               AND mailbox_id IN (SELECT id FROM mailboxes WHERE user_address = $3)",
        )
        .bind(FLAG_SEEN as i32)
        .bind(thread_id)
        .bind(user)
        .bind(new_modseq.0)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as u32)
    }

    /// backfill threading data for messages that have empty thread_id
    /// reads raw bytes from maildir to extract Message-ID/In-Reply-To
    pub async fn backfill_threading(&self, maildir_root: &str) -> u32 {
        // find all messages missing thread_id
        let entries = sqlx::query_as::<_, (i64, String, String)>(
            "SELECT m.id, m.maildir_id, mb.user_address
             FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE m.thread_id = ''",
        )
        .fetch_all(&self.pool)
        .await;

        let entries = match entries {
            Ok(e) => e,
            Err(_) => return 0,
        };

        let mut count = 0u32;
        for (id, maildir_id, user) in &entries {
            let raw = read_raw_from_maildir(maildir_root, user, maildir_id);
            let Some(data) = raw else { continue };

            let msg_id = threading::extract_message_id(&data);
            if msg_id.is_empty() {
                continue;
            }
            let in_reply_to = threading::extract_in_reply_to(&data);

            // look up parent thread_id
            let parent_tid: Option<String> = if !in_reply_to.is_empty() {
                sqlx::query_as::<_, (String,)>(
                    "SELECT m.thread_id FROM messages m
                     JOIN mailboxes mb ON m.mailbox_id = mb.id
                     WHERE mb.user_address = $1 AND m.message_id = $2 AND m.thread_id != ''
                     LIMIT 1",
                )
                .bind(user)
                .bind(&in_reply_to)
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten()
                .map(|r| r.0)
            } else {
                None
            };

            let thread_id =
                threading::resolve_thread_id(&msg_id, &in_reply_to, |_| parent_tid.clone());

            let _ = sqlx::query(
                "UPDATE messages SET message_id = $1, in_reply_to = $2, thread_id = $3 WHERE id = $4",
            )
            .bind(&msg_id)
            .bind(&in_reply_to)
            .bind(&thread_id)
            .bind(id)
            .execute(&self.pool)
            .await;

            count += 1;
        }
        count
    }

    /// copy a message to another mailbox, returns new UID
    pub async fn copy_message(
        &self,
        user: &str,
        src_mailbox_id: i64,
        uid: u32,
        dst_mailbox_name: &str,
    ) -> Result<Option<u32>, sqlx::Error> {
        copy_message_inner(&self.pool, user, src_mailbox_id, uid, dst_mailbox_name).await
    }

    /// move a message: copy to destination + delete from source
    pub async fn move_message(
        &self,
        user: &str,
        src_mailbox_id: i64,
        uid: u32,
        dst_mailbox_name: &str,
    ) -> Result<Option<u32>, sqlx::Error> {
        let new_uid =
            copy_message_inner(&self.pool, user, src_mailbox_id, uid, dst_mailbox_name).await?;
        if new_uid.is_some() {
            sqlx::query("DELETE FROM messages WHERE mailbox_id = $1 AND uid = $2")
                .bind(src_mailbox_id)
                .bind(uid as i32)
                .execute(&self.pool)
                .await?;
        }
        Ok(new_uid)
    }

    /// search conversations by subject or sender (ILIKE search)
    pub async fn search_conversations(
        &self,
        user: &str,
        query: &str,
        limit: u32,
        category: Option<&str>,
    ) -> Result<Vec<ConversationSummary>, sqlx::Error> {
        let pattern = format!("%{query}%");
        let count_expr = "COUNT(DISTINCT CASE WHEN m.message_id != '' THEN m.message_id ELSE CAST(m.id AS TEXT) END)";
        let unread_expr = "COUNT(DISTINCT CASE WHEN (m.flags & 1) = 0 THEN CASE WHEN m.message_id != '' THEN m.message_id ELSE CAST(m.id AS TEXT) END END)";

        let category_filter = if category.is_some() {
            "AND m.id IN (SELECT ea_inner.message_id FROM email_analysis ea_inner WHERE ea_inner.category = $4)"
        } else {
            ""
        };

        let sql = format!(
            "SELECT m.thread_id, MAX(m.subject), string_agg(DISTINCT m.sender, ','),
                    {count_expr}, {unread_expr}, MAX(m.internal_date),
                    COALESCE((SELECT ea.category FROM email_analysis ea
                              JOIN messages m2 ON ea.message_id = m2.id
                              WHERE m2.thread_id = m.thread_id
                              ORDER BY m2.internal_date DESC LIMIT 1), 'general')
             FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND thread_id != ''
               AND (m.subject ILIKE $2 OR m.sender ILIKE $2)
               {category_filter}
             GROUP BY m.thread_id ORDER BY MAX(m.internal_date) DESC LIMIT $3"
        );

        let mut q = sqlx::query_as::<_, (String, Option<String>, Option<String>, i64, i64, i64, String)>(&sql)
            .bind(user)
            .bind(&pattern)
            .bind(limit as i64);

        if let Some(cat) = category {
            q = q.bind(cat);
        }

        let rows = q.fetch_all(&self.pool).await?;

        Ok(rows
            .into_iter()
            .map(|r| ConversationSummary {
                thread_id: r.0,
                subject: r.1.unwrap_or_default(),
                participants: r.2.unwrap_or_default(),
                message_count: r.3 as u32,
                unread_count: r.4 as u32,
                last_date: r.5,
                category: r.6,
            })
            .collect())
    }

    /// list distinct categories with conversation counts
    pub async fn list_conversation_categories(
        &self,
        user: &str,
    ) -> Result<Vec<(String, i64)>, sqlx::Error> {
        let rows = sqlx::query_as::<_, (String, i64)>(
            "SELECT ea.category, COUNT(DISTINCT m.thread_id)
             FROM email_analysis ea
             JOIN messages m ON ea.message_id = m.id
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND m.thread_id != ''
             GROUP BY ea.category
             ORDER BY COUNT(DISTINCT m.thread_id) DESC",
        )
        .bind(user)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// get distinct senders (contacts) matching a query
    pub async fn search_contacts(
        &self,
        user: &str,
        query: &str,
        limit: u32,
    ) -> Result<Vec<String>, sqlx::Error> {
        let pattern = format!("%{query}%");
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT sender FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND sender ILIKE $2 AND sender != ''
             GROUP BY sender
             ORDER BY MAX(internal_date) DESC LIMIT $3",
        )
        .bind(user)
        .bind(&pattern)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    // ---- email analysis methods ----

    /// get analysis result for a message
    pub async fn get_email_analysis(&self, message_id: i64) -> Result<Option<crate::types::EmailAnalysisRow>, sqlx::Error> {
        let row = sqlx::query_as::<_, (i64, String, i16, String, String, serde_json::Value, serde_json::Value, serde_json::Value, serde_json::Value, String, String)>(
            "SELECT message_id, category, risk_score, risk_reason, summary, people, dates, amounts, action_items, model_version, clean_text
             FROM email_analysis WHERE message_id = $1",
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| crate::types::EmailAnalysisRow {
            message_id: r.0,
            category: r.1,
            risk_score: r.2,
            risk_reason: r.3,
            summary: r.4,
            people: r.5,
            dates: r.6,
            amounts: r.7,
            action_items: r.8,
            model_version: r.9,
            clean_text: r.10,
        }))
    }

    /// upsert analysis result
    pub async fn upsert_email_analysis(
        &self,
        message_id: i64,
        category: &str,
        risk_score: i16,
        risk_reason: &str,
        summary: &str,
        people: &serde_json::Value,
        dates: &serde_json::Value,
        amounts: &serde_json::Value,
        action_items: &serde_json::Value,
        embedding: Option<&[f32]>,
        model_version: &str,
        clean_text: &str,
    ) -> Result<(), sqlx::Error> {
        // format embedding as pgvector text literal
        let embedding_str = embedding.map(|v| {
            let nums: Vec<String> = v.iter().map(|f| f.to_string()).collect();
            format!("[{}]", nums.join(","))
        });

        sqlx::query(
            "INSERT INTO email_analysis (message_id, category, risk_score, risk_reason, summary, people, dates, amounts, action_items, embedding, model_version, clean_text, analyzed_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::vector, $11, $12, now())
             ON CONFLICT (message_id) DO UPDATE SET
               category = EXCLUDED.category,
               risk_score = EXCLUDED.risk_score,
               risk_reason = EXCLUDED.risk_reason,
               summary = EXCLUDED.summary,
               people = EXCLUDED.people,
               dates = EXCLUDED.dates,
               amounts = EXCLUDED.amounts,
               action_items = EXCLUDED.action_items,
               embedding = EXCLUDED.embedding,
               model_version = EXCLUDED.model_version,
               clean_text = EXCLUDED.clean_text,
               analyzed_at = now()",
        )
        .bind(message_id)
        .bind(category)
        .bind(risk_score)
        .bind(risk_reason)
        .bind(summary)
        .bind(people)
        .bind(dates)
        .bind(amounts)
        .bind(action_items)
        .bind(embedding_str.as_deref())
        .bind(model_version)
        .bind(clean_text)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// list message IDs that haven't been analyzed yet or need reanalysis (outdated model_version)
    pub async fn list_unanalyzed_message_ids(&self, limit: i64, current_version: &str) -> Result<Vec<(i64, String, String, String, String)>, sqlx::Error> {
        // returns (message_id, user_address, maildir_id, sender, subject)
        let rows = sqlx::query_as::<_, (i64, String, String, String, String)>(
            "SELECT m.id, mb.user_address, m.maildir_id, m.sender, m.subject
             FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             LEFT JOIN email_analysis ea ON m.id = ea.message_id
             WHERE ea.message_id IS NULL OR ea.model_version != $2
             ORDER BY m.id DESC
             LIMIT $1",
        )
        .bind(limit)
        .bind(current_version)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// count total messages needing analysis (unanalyzed + outdated version)
    pub async fn count_unanalyzed_messages(&self, current_version: &str) -> Result<i64, sqlx::Error> {
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*)
             FROM messages m
             LEFT JOIN email_analysis ea ON m.id = ea.message_id
             WHERE ea.message_id IS NULL OR ea.model_version != $1",
        )
        .bind(current_version)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    /// semantic search using pgvector cosine similarity
    pub async fn semantic_search(
        &self,
        user: &str,
        query_embedding: &[f32],
        limit: i64,
    ) -> Result<Vec<(i64, String, f64)>, sqlx::Error> {
        // returns (message_id, thread_id, similarity_score)
        let embedding_str = {
            let nums: Vec<String> = query_embedding.iter().map(|f| f.to_string()).collect();
            format!("[{}]", nums.join(","))
        };

        let rows = sqlx::query_as::<_, (i64, String, f64)>(
            "SELECT m.id, m.thread_id,
                    1 - (ea.embedding <=> $1::vector) AS similarity
             FROM email_analysis ea
             JOIN messages m ON ea.message_id = m.id
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $2
               AND ea.embedding IS NOT NULL
             ORDER BY ea.embedding <=> $1::vector
             LIMIT $3",
        )
        .bind(&embedding_str)
        .bind(user)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }
}

// ---- free functions ----

/// bump highest_modseq for mailbox and return the new value
async fn bump_modseq(pool: &PgPool, mailbox_id: i64) -> Result<u64, sqlx::Error> {
    let row = sqlx::query_as::<_, (i64,)>(
        "UPDATE mailboxes SET highest_modseq = highest_modseq + 1
         WHERE id = $1 RETURNING highest_modseq",
    )
    .bind(mailbox_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0 as u64)
}

/// copy message logic extracted as a free function so both copy_message and move_message can call it
async fn copy_message_inner(
    pool: &PgPool,
    user: &str,
    src_mailbox_id: i64,
    uid: u32,
    dst_mailbox_name: &str,
) -> Result<Option<u32>, sqlx::Error> {
    // read source message (including flags to preserve on copy)
    let src = sqlx::query_as::<_, (String, String, String, String, i32, i64, i64, i32, String, String, String)>(
        "SELECT maildir_id, sender, recipients, subject, size, date_epoch, internal_date, flags, message_id, in_reply_to, thread_id
         FROM messages WHERE mailbox_id = $1 AND uid = $2",
    )
    .bind(src_mailbox_id)
    .bind(uid as i32)
    .fetch_optional(pool)
    .await?;

    let src = match src {
        Some(s) => s,
        None => return Ok(None),
    };

    let mut tx = pool.begin().await?;

    // lock destination mailbox row
    let (dst_id, dst_uidnext, dst_modseq) = sqlx::query_as::<_, (i64, i32, i64)>(
        "SELECT id, uidnext, highest_modseq FROM mailboxes
         WHERE user_address = $1 AND name = $2 FOR UPDATE",
    )
    .bind(user)
    .bind(dst_mailbox_name)
    .fetch_one(&mut *tx)
    .await?;

    let new_uid = dst_uidnext;
    let new_modseq = dst_modseq + 1;

    sqlx::query(
        "INSERT INTO messages (mailbox_id, uid, maildir_id, sender, recipients, subject, size, date_epoch, internal_date, flags, message_id, in_reply_to, thread_id, modseq)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(dst_id)
    .bind(new_uid)
    .bind(&src.0)   // maildir_id
    .bind(&src.1)   // sender
    .bind(&src.2)   // recipients
    .bind(&src.3)   // subject
    .bind(src.4)    // size
    .bind(src.5)    // date_epoch
    .bind(src.6)    // internal_date
    .bind(src.7)    // flags
    .bind(&src.8)   // message_id
    .bind(&src.9)   // in_reply_to
    .bind(&src.10)  // thread_id
    .bind(new_modseq)
    .execute(&mut *tx)
    .await?;

    sqlx::query("UPDATE mailboxes SET uidnext = $1, highest_modseq = $2 WHERE id = $3")
        .bind(new_uid + 1)
        .bind(new_modseq)
        .bind(dst_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    Ok(Some(new_uid as u32))
}

/// convert a tuple row to MessageMeta
fn row_to_message_meta(
    r: (i64, i64, i32, String, String, String, String, i64, i32, i32, i64, String, String, String, i64),
) -> MessageMeta {
    MessageMeta {
        id: r.0,
        mailbox_id: r.1,
        uid: r.2 as u32,
        maildir_id: r.3,
        sender: r.4,
        recipients: r.5,
        subject: r.6,
        date: r.7,
        size: r.8 as u32,
        flags: r.9 as u32,
        internal_date: r.10,
        message_id: r.11,
        in_reply_to: r.12,
        thread_id: r.13,
        modseq: r.14 as u64,
    }
}

/// extract a raw header value from RFC 5322 message bytes
fn extract_header_value(data: &[u8], name: &str) -> String {
    let text = String::from_utf8_lossy(data);
    let prefix = format!("{name}:");
    for line in text.lines() {
        if line.len() > prefix.len() && line[..prefix.len()].eq_ignore_ascii_case(&prefix) {
            return line[prefix.len()..].trim().to_string();
        }
        if line.is_empty() {
            break;
        }
    }
    String::new()
}

/// read raw message bytes from maildir
fn read_raw_from_maildir(maildir_root: &str, user: &str, maildir_id: &str) -> Option<Vec<u8>> {
    let (local, domain) = user.split_once('@')?;
    let path = format!("{maildir_root}/{domain}/{local}");
    let md = mailrs_storage_maildir::Maildir::open(&path);

    let find_in = |entries: Vec<mailrs_storage_maildir::Entry>| -> Option<Vec<u8>> {
        entries
            .into_iter()
            .find(|e| e.id.to_string() == maildir_id)
            .and_then(|e| std::fs::read(&e.path).ok())
    };

    find_in(md.scan_cur().unwrap_or_default())
        .or_else(|| find_in(md.scan_new().unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_header_value_basic() {
        let msg = b"From: alice@example.com\r\nSubject: Hello World\r\n\r\nbody";
        assert_eq!(extract_header_value(msg, "Subject"), "Hello World");
        assert_eq!(extract_header_value(msg, "From"), "alice@example.com");
    }

    #[test]
    fn extract_header_value_case_insensitive() {
        let msg = b"subject: hello world\r\n\r\n";
        assert_eq!(extract_header_value(msg, "Subject"), "hello world");
    }

    #[test]
    fn extract_header_value_missing() {
        let msg = b"From: alice@example.com\r\n\r\nbody";
        assert_eq!(extract_header_value(msg, "Subject"), "");
    }

    #[test]
    fn extract_header_value_stops_at_empty_line() {
        let msg = b"From: alice@example.com\r\n\r\nSubject: in body";
        assert_eq!(extract_header_value(msg, "Subject"), "");
    }

    #[test]
    fn extract_header_value_trims_whitespace() {
        let msg = b"Subject:   lots of spaces   \r\n\r\n";
        assert_eq!(extract_header_value(msg, "Subject"), "lots of spaces");
    }

    #[test]
    fn extract_header_value_empty_message() {
        assert_eq!(extract_header_value(b"", "Subject"), "");
    }
}
