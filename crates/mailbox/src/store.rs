use sqlx::{PgPool, Row};

use crate::threading;
use crate::types::{ConversationSummary, FlagAction, Mailbox, MessageMeta, FLAG_DELETED, FLAG_FLAGGED, FLAG_SEEN};

/// build a user_address filter clause and collect bind values
/// returns (sql_fragment, bind_values) where bind values start at `start_idx`
fn build_user_filter(user: &str, domains: Option<&[String]>, start_idx: u32) -> (String, Vec<String>) {
    if let Some(doms) = domains {
        if !doms.is_empty() {
            let placeholders: Vec<String> = doms.iter().enumerate()
                .map(|(i, _)| format!("${}", start_idx + i as u32))
                .collect();
            let sql = format!(
                "mb.user_address IN (SELECT address FROM accounts WHERE domain IN ({}))",
                placeholders.join(",")
            );
            return (sql, doms.to_vec());
        }
    }
    (format!("mb.user_address = ${start_idx}"), vec![user.to_string()])
}

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
    pub async fn count_messages(&self, user: &str) -> i64 {
        let row: Result<(i64,), _> = sqlx::query_as(
            "SELECT COUNT(*) FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id WHERE mb.user_address = $1",
        )
        .bind(user)
        .fetch_one(&self.pool)
        .await;

        row.map(|r| r.0).unwrap_or(0)
    }

    pub async fn count_unseen(&self, user: &str) -> i64 {
        let row: Result<(i64,), _> = sqlx::query_as(
            "SELECT COUNT(*) FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND m.flags & 1 = 0",
        )
        .bind(user)
        .fetch_one(&self.pool)
        .await;

        row.map(|r| r.0).unwrap_or(0)
    }

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
    /// when `domains` is Some, query across all accounts in those domains instead of single user
    pub async fn list_conversations(
        &self,
        user: &str,
        limit: u32,
        before_ts: Option<i64>,
        category: Option<&str>,
        domains: Option<&[String]>,
        archived: bool,
        folder: Option<&str>,
    ) -> Result<Vec<ConversationSummary>, sqlx::Error> {
        let count_expr = "COUNT(DISTINCT CASE WHEN m.message_id != '' THEN m.message_id ELSE CAST(m.id AS TEXT) END)";
        let unread_expr = "COUNT(DISTINCT CASE WHEN (m.flags & 1) = 0 THEN CASE WHEN m.message_id != '' THEN m.message_id ELSE CAST(m.id AS TEXT) END END)";

        // build dynamic WHERE clauses
        let archived_filter = if archived {
            "BOOL_OR(m.archived) = true"
        } else {
            "BOOL_OR(m.archived) = false"
        };
        let mut conditions = vec!["thread_id != ''".to_string()];
        let mut param_idx = 1u32;

        // user filter: either single user or multi-domain
        let user_condition = if let Some(doms) = domains {
            if doms.is_empty() {
                param_idx += 1;
                format!("mb.user_address = ${}", param_idx - 1)
            } else {
                let placeholders: Vec<String> = doms.iter().enumerate().map(|(i, _)| format!("${}", param_idx + i as u32)).collect();
                param_idx += doms.len() as u32;
                format!("mb.user_address IN (SELECT address FROM accounts WHERE domain IN ({}))", placeholders.join(","))
            }
        } else {
            param_idx += 1;
            format!("mb.user_address = ${}", param_idx - 1)
        };
        conditions.insert(0, user_condition);

        // exclude snoozed conversations (snooze still active)
        conditions.push(format!(
            "NOT EXISTS (SELECT 1 FROM snoozed_conversations sc WHERE sc.thread_id = m.thread_id AND sc.account_address = mb.user_address AND sc.snoozed_until > NOW())"
        ));

        // folder filter (e.g. "Sent", "Drafts")
        if folder.is_some() {
            conditions.push(format!("mb.name = ${param_idx}"));
            param_idx += 1;
        }

        let limit_idx = param_idx;
        param_idx += 1;

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
                              ORDER BY m2.internal_date DESC LIMIT 1), 'general'),
                    BOOL_OR((m.flags & 4) != 0),
                    COALESCE((SELECT LEFT(m3.text_body, 120) FROM messages m3
                              WHERE m3.thread_id = m.thread_id AND m3.text_body IS NOT NULL AND m3.text_body != ''
                              ORDER BY m3.internal_date DESC LIMIT 1), ''),
                    BOOL_OR(m.pinned),
                    BOOL_OR(m.archived),
                    COALESCE((SELECT m_imp.importance_level FROM messages m_imp WHERE m_imp.thread_id = m.thread_id ORDER BY m_imp.importance_score DESC NULLS LAST LIMIT 1), 'normal'),
                    COALESCE(MAX(m.importance_score), 0.0)
             FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE {where_clause}
             GROUP BY m.thread_id HAVING {archived_filter}
             ORDER BY BOOL_OR(m.pinned) DESC, MAX(m.internal_date) DESC LIMIT ${limit_idx}"
        );

        // bind parameters in order
        let mut query = sqlx::query_as::<_, (String, Option<String>, Option<String>, i64, i64, i64, String, bool, String, bool, bool, String, f32)>(&sql);

        if let Some(doms) = domains {
            if doms.is_empty() {
                query = query.bind(user);
            } else {
                for d in doms {
                    query = query.bind(d);
                }
            }
        } else {
            query = query.bind(user);
        }

        if let Some(f) = folder {
            query = query.bind(f);
        }

        query = query.bind(limit as i64);

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
                flagged: r.7,
                snippet: r.8,
                pinned: r.9,
                archived: r.10,
                importance_level: r.11,
                importance_score: r.12,
            })
            .collect())
    }

    /// list all messages in a thread (deduplicated by message_id)
    /// when `domains` is Some, query across all accounts in those domains
    pub async fn list_thread_messages(
        &self,
        user: &str,
        thread_id: &str,
        domains: Option<&[String]>,
    ) -> Result<Vec<MessageMeta>, sqlx::Error> {
        // deduplicate: same email may exist in both INBOX and Sent
        let (user_filter, user_filter_inner) = if let Some(doms) = domains {
            if !doms.is_empty() {
                let placeholders: Vec<String> = doms.iter().enumerate().map(|(i, _)| format!("${}", i + 3)).collect();
                let f = format!("mb.user_address IN (SELECT address FROM accounts WHERE domain IN ({}))", placeholders.join(","));
                let f2 = format!("mb2.user_address IN (SELECT address FROM accounts WHERE domain IN ({}))", placeholders.join(","));
                (f, f2)
            } else {
                ("mb.user_address = $1".to_string(), "mb2.user_address = $1".to_string())
            }
        } else {
            ("mb.user_address = $1".to_string(), "mb2.user_address = $1".to_string())
        };

        let sql = format!(
            "SELECT m.id, m.mailbox_id, m.uid, m.maildir_id, m.sender, m.recipients, m.subject,
                    m.date_epoch, m.size, m.flags, m.internal_date, m.message_id, m.in_reply_to, m.thread_id, m.modseq,
                    mb.user_address,
                    COALESCE(m.importance_level, 'normal'), COALESCE(m.importance_score, 0.0),
                    COALESCE(m.is_bulk_sender, false), COALESCE(m.has_tracking_pixel, false),
                    m.new_content
             FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE {user_filter} AND m.thread_id = $2
               AND m.id = (
                 SELECT MIN(m2.id) FROM messages m2
                 JOIN mailboxes mb2 ON m2.mailbox_id = mb2.id
                 WHERE {user_filter_inner}
                   AND CASE WHEN m.message_id != '' THEN m2.message_id = m.message_id
                            ELSE m2.id = m.id END
               )
             ORDER BY m.internal_date ASC"
        );

        let mut query = sqlx::query(&sql)
            .bind(user)
            .bind(thread_id);

        if let Some(doms) = domains {
            if !doms.is_empty() {
                for d in doms {
                    query = query.bind(d);
                }
            }
        }

        let rows = query.fetch_all(&self.pool).await?;

        Ok(rows.into_iter().map(row_to_message_meta_from_row).collect())
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

    /// get the message_id of the last message in a thread (by internal_date)
    pub async fn get_last_message_id_in_thread(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT m.message_id FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND m.thread_id = $2 AND m.message_id != ''
             ORDER BY m.internal_date DESC
             LIMIT 1",
        )
        .bind(user)
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.0))
    }

    /// get all message_ids in a thread ordered by date (for References header)
    pub async fn get_thread_message_ids(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT DISTINCT m.message_id
             FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND m.thread_id = $2 AND m.message_id != ''
             ORDER BY m.message_id",
        )
        .bind(user)
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// mark all messages in a thread as read
    /// when `domains` is provided, marks read across all accounts in those domains
    pub async fn mark_thread_read(
        &self,
        user: &str,
        thread_id: &str,
        domains: Option<&[String]>,
    ) -> Result<u32, sqlx::Error> {
        // determine user filter and param count
        let (user_filter, extra_params) =
            if let Some(doms) = domains.filter(|d| !d.is_empty()) {
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
    pub async fn unsnooze_thread(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<(), sqlx::Error> {
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
        domains: Option<&[String]>,
    ) -> Result<Vec<ConversationSummary>, sqlx::Error> {
        let escaped = query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        let pattern = format!("%{escaped}%");
        let count_expr = "COUNT(DISTINCT CASE WHEN m.message_id != '' THEN m.message_id ELSE CAST(m.id AS TEXT) END)";
        let unread_expr = "COUNT(DISTINCT CASE WHEN (m.flags & 1) = 0 THEN CASE WHEN m.message_id != '' THEN m.message_id ELSE CAST(m.id AS TEXT) END END)";

        // build param indices dynamically
        let mut param_idx = 1u32;

        let user_filter = if let Some(doms) = domains {
            if !doms.is_empty() {
                let placeholders: Vec<String> = doms.iter().enumerate().map(|(i, _)| format!("${}", param_idx + i as u32)).collect();
                param_idx += doms.len() as u32;
                format!("mb.user_address IN (SELECT address FROM accounts WHERE domain IN ({}))", placeholders.join(","))
            } else {
                let f = format!("mb.user_address = ${param_idx}");
                param_idx += 1;
                f
            }
        } else {
            let f = format!("mb.user_address = ${param_idx}");
            param_idx += 1;
            f
        };

        let pattern_idx = param_idx;
        param_idx += 1;
        let limit_idx = param_idx;
        param_idx += 1;

        let category_filter = if category.is_some() {
            format!("AND m.id IN (SELECT ea_inner.message_id FROM email_analysis ea_inner WHERE ea_inner.category = ${param_idx})")
        } else {
            String::new()
        };

        let sql = format!(
            "SELECT m.thread_id, MAX(m.subject), string_agg(DISTINCT m.sender, ','),
                    {count_expr}, {unread_expr}, MAX(m.internal_date),
                    COALESCE((SELECT ea.category FROM email_analysis ea
                              JOIN messages m2 ON ea.message_id = m2.id
                              WHERE m2.thread_id = m.thread_id
                              ORDER BY m2.internal_date DESC LIMIT 1), 'general'),
                    BOOL_OR((m.flags & 4) != 0),
                    COALESCE((SELECT LEFT(m3.text_body, 120) FROM messages m3
                              WHERE m3.thread_id = m.thread_id AND m3.text_body IS NOT NULL AND m3.text_body != ''
                              ORDER BY m3.internal_date DESC LIMIT 1), ''),
                    BOOL_OR(m.pinned),
                    BOOL_OR(m.archived),
                    COALESCE((SELECT m_imp.importance_level FROM messages m_imp WHERE m_imp.thread_id = m.thread_id ORDER BY m_imp.importance_score DESC NULLS LAST LIMIT 1), 'normal'),
                    COALESCE(MAX(m.importance_score), 0.0)
             FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE {user_filter} AND thread_id != ''
               AND (m.subject ILIKE ${pattern_idx} OR m.sender ILIKE ${pattern_idx}
                    OR m.text_body ILIKE ${pattern_idx}
                    OR m.clean_text ILIKE ${pattern_idx}
                    OR EXISTS (SELECT 1 FROM attachment_content ac WHERE ac.message_id = m.id AND ac.extracted_text ILIKE ${pattern_idx}))
               {category_filter}
             GROUP BY m.thread_id HAVING BOOL_OR(m.archived) = false
             ORDER BY MAX(m.internal_date) DESC LIMIT ${limit_idx}"
        );

        let mut q = sqlx::query_as::<_, (String, Option<String>, Option<String>, i64, i64, i64, String, bool, String, bool, bool, String, f32)>(&sql);

        if let Some(doms) = domains {
            if !doms.is_empty() {
                for d in doms {
                    q = q.bind(d);
                }
            } else {
                q = q.bind(user);
            }
        } else {
            q = q.bind(user);
        }

        q = q.bind(&pattern).bind(limit as i64);

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
                flagged: r.7,
                snippet: r.8,
                pinned: r.9,
                archived: r.10,
                importance_level: r.11,
                importance_score: r.12,
            })
            .collect())
    }

    /// list distinct categories with conversation counts
    pub async fn list_conversation_categories(
        &self,
        user: &str,
        domains: Option<&[String]>,
    ) -> Result<Vec<(String, i64)>, sqlx::Error> {
        let (user_filter, binds_domains) = build_user_filter(user, domains, 1);

        let sql = format!(
            "SELECT ea.category, COUNT(DISTINCT m.thread_id)
             FROM email_analysis ea
             JOIN messages m ON ea.message_id = m.id
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE {user_filter} AND m.thread_id != ''
             GROUP BY ea.category
             ORDER BY COUNT(DISTINCT m.thread_id) DESC"
        );

        let mut query = sqlx::query_as::<_, (String, i64)>(&sql);
        for b in &binds_domains {
            query = query.bind(b);
        }

        let rows = query.fetch_all(&self.pool).await?;
        Ok(rows)
    }

    /// get distinct senders (contacts) matching a query
    pub async fn search_contacts(
        &self,
        user: &str,
        query: &str,
        limit: u32,
    ) -> Result<Vec<String>, sqlx::Error> {
        let escaped = query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        let pattern = format!("%{escaped}%");
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
        let row = sqlx::query_as::<_, (i64, String, i16, String, String, serde_json::Value, serde_json::Value, serde_json::Value, serde_json::Value, String, String, bool, String, Option<String>)>(
            "SELECT message_id, category, risk_score, risk_reason, summary, people, dates, amounts, action_items, model_version,
                    COALESCE(clean_text, ''), COALESCE(requires_action, false), COALESCE(sender_intent, 'inform'),
                    action_deadline::text
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
            requires_action: r.11,
            sender_intent: r.12,
            action_deadline: r.13,
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
        requires_action: bool,
        sender_intent: &str,
        action_deadline: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        // format embedding as pgvector text literal
        let embedding_str = embedding.map(|v| {
            let nums: Vec<String> = v.iter().map(|f| f.to_string()).collect();
            format!("[{}]", nums.join(","))
        });

        sqlx::query(
            "INSERT INTO email_analysis (message_id, category, risk_score, risk_reason, summary, people, dates, amounts, action_items, embedding, model_version, clean_text, requires_action, sender_intent, action_deadline, analyzed_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::vector, $11, $12, $13, $14, $15::timestamptz, now())
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
               requires_action = EXCLUDED.requires_action,
               sender_intent = EXCLUDED.sender_intent,
               action_deadline = EXCLUDED.action_deadline,
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
        .bind(requires_action)
        .bind(sender_intent)
        .bind(action_deadline)
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
        domains: Option<&[String]>,
    ) -> Result<Vec<(i64, String, f64)>, sqlx::Error> {
        // returns (message_id, thread_id, similarity_score)
        let embedding_str = {
            let nums: Vec<String> = query_embedding.iter().map(|f| f.to_string()).collect();
            format!("[{}]", nums.join(","))
        };

        // $1 = embedding, user_filter starts at $2, limit is after
        let (user_filter, binds) = build_user_filter(user, domains, 2);
        let limit_idx = 2 + binds.len() as u32;

        let sql = format!(
            "SELECT m.id, m.thread_id,
                    1 - (ea.embedding <=> $1::vector) AS similarity
             FROM email_analysis ea
             JOIN messages m ON ea.message_id = m.id
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE {user_filter}
               AND ea.embedding IS NOT NULL
             ORDER BY ea.embedding <=> $1::vector
             LIMIT ${limit_idx}"
        );

        let mut query = sqlx::query_as::<_, (i64, String, f64)>(&sql)
            .bind(&embedding_str);
        for b in &binds {
            query = query.bind(b);
        }
        query = query.bind(limit);

        let rows = query.fetch_all(&self.pool).await?;

        Ok(rows)
    }

    // ---- contact management ----

    /// upsert a contact on inbound email (received from sender)
    pub async fn upsert_contact_inbound(
        &self,
        user: &str,
        sender_email: &str,
        display_name: &str,
        is_mailing_list: bool,
        is_automated: bool,
    ) -> Result<(), sqlx::Error> {
        let email = normalize_email(sender_email);
        sqlx::query(
            "INSERT INTO contacts (user_address, email, display_name, first_seen, last_seen, received_count, is_mailing_list, is_automated)
             VALUES ($1, $2, $3, now(), now(), 1, $4, $5)
             ON CONFLICT (user_address, email) DO UPDATE SET
               display_name = CASE WHEN EXCLUDED.display_name != '' THEN EXCLUDED.display_name ELSE contacts.display_name END,
               last_seen = now(),
               received_count = contacts.received_count + 1,
               is_mailing_list = contacts.is_mailing_list OR EXCLUDED.is_mailing_list,
               is_automated = contacts.is_automated OR EXCLUDED.is_automated",
        )
        .bind(user)
        .bind(&email)
        .bind(display_name)
        .bind(is_mailing_list)
        .bind(is_automated)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// upsert a contact on outbound email (sent to recipient)
    pub async fn upsert_contact_outbound(
        &self,
        user: &str,
        recipient_email: &str,
        display_name: &str,
    ) -> Result<(), sqlx::Error> {
        let email = normalize_email(recipient_email);
        sqlx::query(
            "INSERT INTO contacts (user_address, email, display_name, first_seen, last_seen, sent_count, is_mutual)
             VALUES ($1, $2, $3, now(), now(), 1, true)
             ON CONFLICT (user_address, email) DO UPDATE SET
               display_name = CASE WHEN EXCLUDED.display_name != '' THEN EXCLUDED.display_name ELSE contacts.display_name END,
               last_seen = now(),
               sent_count = contacts.sent_count + 1,
               is_mutual = true",
        )
        .bind(user)
        .bind(&email)
        .bind(display_name)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// mark contact as mutual (when user replies to a sender)
    pub async fn mark_contact_mutual(
        &self,
        user: &str,
        email: &str,
    ) -> Result<(), sqlx::Error> {
        let email = normalize_email(email);
        sqlx::query(
            "UPDATE contacts SET is_mutual = true, reply_count = reply_count + 1
             WHERE user_address = $1 AND email = $2",
        )
        .bind(user)
        .bind(&email)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// get contact info for importance scoring
    pub async fn get_contact_for_scoring(
        &self,
        user: &str,
        sender_email: &str,
    ) -> Result<Option<ContactInfo>, sqlx::Error> {
        let email = normalize_email(sender_email);
        let row = sqlx::query_as::<_, (bool, bool, bool, bool, f32, i32, i32)>(
            "SELECT is_mutual, is_mailing_list, is_vip, is_blocked, importance_bias, received_count, sent_count
             FROM contacts WHERE user_address = $1 AND email = $2",
        )
        .bind(user)
        .bind(&email)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| ContactInfo {
            is_mutual: r.0,
            is_mailing_list: r.1,
            is_vip: r.2,
            is_blocked: r.3,
            importance_bias: r.4,
            received_count: r.5,
            sent_count: r.6,
        }))
    }

    /// update message content fields after deep cleaning
    pub async fn update_message_content(
        &self,
        message_id: i64,
        text_body: Option<&str>,
        html_body: Option<&str>,
        clean_text: Option<&str>,
        new_content: Option<&str>,
        is_bulk_sender: bool,
        has_tracking_pixel: bool,
        importance_level: &str,
        importance_score: f32,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE messages SET
               text_body = COALESCE($2, text_body),
               html_body = COALESCE($3, html_body),
               clean_text = COALESCE($4, clean_text),
               new_content = COALESCE($5, new_content),
               is_bulk_sender = $6,
               has_tracking_pixel = $7,
               importance_level = $8,
               importance_score = $9
             WHERE id = $1",
        )
        .bind(message_id)
        .bind(text_body)
        .bind(html_body)
        .bind(clean_text)
        .bind(new_content)
        .bind(is_bulk_sender)
        .bind(has_tracking_pixel)
        .bind(importance_level)
        .bind(importance_score)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// boost importance score when AI detects action items
    pub async fn boost_importance_for_action(&self, message_id: i64) -> Result<(), sqlx::Error> {
        // add 0.2 to importance_score and re-evaluate level
        sqlx::query(
            "UPDATE messages SET
               importance_score = LEAST(1.0, importance_score + 0.2),
               importance_level = CASE
                 WHEN LEAST(1.0, importance_score + 0.2) >= 0.8 THEN 'critical'
                 WHEN LEAST(1.0, importance_score + 0.2) >= 0.5 THEN 'important'
                 WHEN LEAST(1.0, importance_score + 0.2) >= 0.2 THEN 'normal'
                 WHEN LEAST(1.0, importance_score + 0.2) >= 0.0 THEN 'low'
                 ELSE 'noise'
               END
             WHERE id = $1",
        )
        .bind(message_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// get message id by mailbox user and maildir_id
    pub async fn get_message_id_by_maildir(
        &self,
        user: &str,
        maildir_id: &str,
    ) -> Result<Option<i64>, sqlx::Error> {
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT m.id FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND m.maildir_id = $2",
        )
        .bind(user)
        .bind(maildir_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.0))
    }

    /// check if user has sent email to this address (for is_reply_to_my_email detection)
    pub async fn has_sent_to(
        &self,
        user: &str,
        recipient_email: &str,
    ) -> Result<bool, sqlx::Error> {
        let email = normalize_email(recipient_email);
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM contacts
             WHERE user_address = $1 AND email = $2 AND sent_count > 0",
        )
        .bind(user)
        .bind(&email)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0 > 0)
    }

    /// record user feedback on a sender (for learning)
    pub async fn record_sender_feedback(
        &self,
        user: &str,
        sender_email: &str,
        action: &str,
    ) -> Result<(), sqlx::Error> {
        let email = normalize_email(sender_email);
        sqlx::query(
            "INSERT INTO sender_feedback (user_address, sender_email, action) VALUES ($1, $2, $3)",
        )
        .bind(user)
        .bind(&email)
        .bind(action)
        .execute(&self.pool)
        .await?;

        // update contact importance_bias based on action
        let bias_delta: f32 = match action {
            "mark_important" => 0.2,
            "mark_vip" => 0.4,
            "mark_spam" | "block" => -0.5,
            "unblock" => 0.5,
            "archive" => -0.05,
            _ => 0.0,
        };

        if bias_delta.abs() > f32::EPSILON {
            sqlx::query(
                "UPDATE contacts SET importance_bias = LEAST(1.0, GREATEST(-1.0, importance_bias + $3))
                 WHERE user_address = $1 AND email = $2",
            )
            .bind(user)
            .bind(&email)
            .bind(bias_delta)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }
}

/// contact info for importance scoring
pub struct ContactInfo {
    pub is_mutual: bool,
    pub is_mailing_list: bool,
    pub is_vip: bool,
    pub is_blocked: bool,
    pub importance_bias: f32,
    pub received_count: i32,
    pub sent_count: i32,
}

/// normalize email address: lowercase, remove +tags
fn normalize_email(email: &str) -> String {
    let email = email.trim().to_lowercase();
    // extract bare email from "Display Name <email@domain>" format
    let email = if let Some(start) = email.find('<') {
        if let Some(end) = email.find('>') {
            email[start + 1..end].to_string()
        } else {
            email
        }
    } else {
        email
    };

    // remove + tags (e.g., user+tag@domain -> user@domain)
    if let Some((local, domain)) = email.split_once('@') {
        let local = if let Some((base, _)) = local.split_once('+') {
            base
        } else {
            local
        };
        format!("{local}@{domain}")
    } else {
        email
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
        user_address: String::new(),
        importance_level: String::from("normal"),
        importance_score: 0.0,
        is_bulk_sender: false,
        has_tracking_pixel: false,
        new_content: None,
    }
}

/// convert a PgRow to MessageMeta (for queries with >16 columns)
fn row_to_message_meta_from_row(r: sqlx::postgres::PgRow) -> MessageMeta {
    MessageMeta {
        id: r.get::<i64, _>(0),
        mailbox_id: r.get::<i64, _>(1),
        uid: r.get::<i32, _>(2) as u32,
        maildir_id: r.get::<String, _>(3),
        sender: r.get::<String, _>(4),
        recipients: r.get::<String, _>(5),
        subject: r.get::<String, _>(6),
        date: r.get::<i64, _>(7),
        size: r.get::<i32, _>(8) as u32,
        flags: r.get::<i32, _>(9) as u32,
        internal_date: r.get::<i64, _>(10),
        message_id: r.get::<String, _>(11),
        in_reply_to: r.get::<String, _>(12),
        thread_id: r.get::<String, _>(13),
        modseq: r.get::<i64, _>(14) as u64,
        user_address: r.get::<String, _>(15),
        importance_level: r.get::<String, _>(16),
        importance_score: r.get::<f32, _>(17),
        is_bulk_sender: r.get::<bool, _>(18),
        has_tracking_pixel: r.get::<bool, _>(19),
        new_content: r.get::<Option<String>, _>(20),
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

impl MailboxStore {
    /// get concatenated extracted text from all attachments of a message
    pub async fn get_attachment_texts(&self, message_id: i64) -> Result<String, sqlx::Error> {
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT COALESCE(extracted_text, '')
             FROM attachment_content
             WHERE message_id = $1 AND attachment_index >= 0 AND extracted_text != ''
             ORDER BY attachment_index",
        )
        .bind(message_id)
        .fetch_all(&self.pool)
        .await?;

        let combined: String = rows
            .into_iter()
            .map(|r| r.0)
            .collect::<Vec<_>>()
            .join("\n---\n");

        Ok(combined)
    }
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

    // ---- build_user_filter tests ----

    #[test]
    fn build_user_filter_no_domains() {
        let (sql, binds) = build_user_filter("alice@example.com", None, 1);
        assert_eq!(sql, "mb.user_address = $1");
        assert_eq!(binds, vec!["alice@example.com"]);
    }

    #[test]
    fn build_user_filter_empty_domains() {
        let (sql, binds) = build_user_filter("alice@example.com", Some(&[]), 1);
        assert_eq!(sql, "mb.user_address = $1");
        assert_eq!(binds, vec!["alice@example.com"]);
    }

    #[test]
    fn build_user_filter_single_domain() {
        let domains = vec!["example.com".to_string()];
        let (sql, binds) = build_user_filter("alice@example.com", Some(&domains), 1);
        assert_eq!(
            sql,
            "mb.user_address IN (SELECT address FROM accounts WHERE domain IN ($1))"
        );
        assert_eq!(binds, vec!["example.com"]);
    }

    #[test]
    fn build_user_filter_multiple_domains() {
        let domains = vec!["a.com".to_string(), "b.com".to_string(), "c.com".to_string()];
        let (sql, binds) = build_user_filter("user@a.com", Some(&domains), 1);
        assert_eq!(
            sql,
            "mb.user_address IN (SELECT address FROM accounts WHERE domain IN ($1,$2,$3))"
        );
        assert_eq!(binds, vec!["a.com", "b.com", "c.com"]);
    }

    #[test]
    fn build_user_filter_custom_start_idx() {
        let (sql, binds) = build_user_filter("alice@example.com", None, 5);
        assert_eq!(sql, "mb.user_address = $5");
        assert_eq!(binds, vec!["alice@example.com"]);

        let domains = vec!["x.com".to_string(), "y.com".to_string()];
        let (sql2, binds2) = build_user_filter("u@x.com", Some(&domains), 3);
        assert_eq!(
            sql2,
            "mb.user_address IN (SELECT address FROM accounts WHERE domain IN ($3,$4))"
        );
        assert_eq!(binds2, vec!["x.com", "y.com"]);
    }

    // ---- row_to_message_meta tests ----

    #[test]
    fn row_to_message_meta_converts_correctly() {
        let row = (
            42i64, 7i64, 100i32,
            "maildir-abc".to_string(),
            "sender@test.com".to_string(),
            "rcpt@test.com".to_string(),
            "Test Subject".to_string(),
            1700000000i64, 2048i32, 1i32, 1700000001i64,
            "<msg-001@test.com>".to_string(),
            "<parent@test.com>".to_string(),
            "thread-xyz".to_string(),
            5i64,
        );
        let meta = row_to_message_meta(row);
        assert_eq!(meta.id, 42);
        assert_eq!(meta.mailbox_id, 7);
        assert_eq!(meta.uid, 100);
        assert_eq!(meta.maildir_id, "maildir-abc");
        assert_eq!(meta.sender, "sender@test.com");
        assert_eq!(meta.recipients, "rcpt@test.com");
        assert_eq!(meta.subject, "Test Subject");
        assert_eq!(meta.date, 1700000000);
        assert_eq!(meta.size, 2048);
        assert_eq!(meta.flags, 1);
        assert_eq!(meta.internal_date, 1700000001);
        assert_eq!(meta.message_id, "<msg-001@test.com>");
        assert_eq!(meta.in_reply_to, "<parent@test.com>");
        assert_eq!(meta.thread_id, "thread-xyz");
        assert_eq!(meta.modseq, 5);
        assert_eq!(meta.user_address, ""); // default empty
    }

    #[test]
    fn row_to_message_meta_defaults() {
        // row_to_message_meta sets default importance fields
        let row = (
            1i64, 2i64, 3i32,
            "mid".to_string(), "s".to_string(), "r".to_string(), "sub".to_string(),
            0i64, 0i32, 0i32, 0i64,
            "".to_string(), "".to_string(), "".to_string(), 0i64,
        );
        let meta = row_to_message_meta(row);
        assert_eq!(meta.user_address, "");
        assert_eq!(meta.importance_level, "normal");
        assert_eq!(meta.importance_score, 0.0);
        assert!(!meta.is_bulk_sender);
        assert!(!meta.has_tracking_pixel);
        assert_eq!(meta.new_content, None);
    }

    // ---- MessageMeta clone/debug tests ----

    #[test]
    fn message_meta_clone() {
        let meta = MessageMeta {
            id: 1, mailbox_id: 2, uid: 3, maildir_id: "abc".into(),
            sender: "s@t.com".into(), recipients: "r@t.com".into(),
            subject: "sub".into(), date: 100, size: 50, flags: FLAG_SEEN,
            internal_date: 101, message_id: "mid".into(),
            in_reply_to: "irt".into(), thread_id: "tid".into(),
            modseq: 42, user_address: "u@t.com".into(),
            importance_level: "normal".into(), importance_score: 0.0,
            is_bulk_sender: false, has_tracking_pixel: false, new_content: None,
        };
        let cloned = meta.clone();
        assert_eq!(cloned.id, meta.id);
        assert_eq!(cloned.subject, meta.subject);
        assert_eq!(cloned.flags, meta.flags);
        assert_eq!(cloned.user_address, meta.user_address);
    }

    #[test]
    fn message_meta_debug() {
        let meta = MessageMeta {
            id: 1, mailbox_id: 2, uid: 3, maildir_id: "abc".into(),
            sender: "s".into(), recipients: "r".into(), subject: "sub".into(),
            date: 0, size: 0, flags: 0, internal_date: 0,
            message_id: "".into(), in_reply_to: "".into(), thread_id: "".into(),
            modseq: 0, user_address: "".into(),
            importance_level: "normal".into(), importance_score: 0.0,
            is_bulk_sender: false, has_tracking_pixel: false, new_content: None,
        };
        let debug = format!("{:?}", meta);
        assert!(debug.contains("MessageMeta"));
        assert!(debug.contains("abc"));
    }

    // ---- ConversationSummary tests ----

    #[test]
    fn conversation_summary_clone() {
        let cs = ConversationSummary {
            thread_id: "t1".into(), subject: "Hello".into(),
            participants: "alice,bob".into(), message_count: 5,
            unread_count: 2, last_date: 1700000000, category: "general".into(),
            flagged: true, snippet: "preview text".into(),
            pinned: false, archived: false,
            importance_level: "normal".into(), importance_score: 0.0,
        };
        let cloned = cs.clone();
        assert_eq!(cloned.thread_id, "t1");
        assert_eq!(cloned.message_count, 5);
        assert_eq!(cloned.unread_count, 2);
        assert!(cloned.flagged);
        assert!(!cloned.pinned);
        assert!(!cloned.archived);
        assert_eq!(cloned.snippet, "preview text");
    }

    #[test]
    fn conversation_summary_debug() {
        let cs = ConversationSummary {
            thread_id: "t1".into(), subject: "Hi".into(),
            participants: "a".into(), message_count: 1,
            unread_count: 0, last_date: 0, category: "promo".into(),
            flagged: false, snippet: "".into(),
            pinned: true, archived: true,
            importance_level: "normal".into(), importance_score: 0.0,
        };
        let debug = format!("{:?}", cs);
        assert!(debug.contains("ConversationSummary"));
        assert!(debug.contains("promo"));
    }

    // ---- Mailbox tests ----

    #[test]
    fn mailbox_clone_and_debug() {
        let mb = Mailbox {
            id: 10, user: "bob@test.com".into(), name: "INBOX".into(),
            uidvalidity: 12345, uidnext: 99, highest_modseq: 50,
        };
        let cloned = mb.clone();
        assert_eq!(cloned.id, 10);
        assert_eq!(cloned.user, "bob@test.com");
        assert_eq!(cloned.name, "INBOX");
        assert_eq!(cloned.uidvalidity, 12345);
        assert_eq!(cloned.uidnext, 99);
        assert_eq!(cloned.highest_modseq, 50);

        let debug = format!("{:?}", mb);
        assert!(debug.contains("Mailbox"));
        assert!(debug.contains("INBOX"));
    }

    // ---- extract_header_value edge cases ----

    #[test]
    fn extract_header_value_no_crlf() {
        let msg = b"Subject: Unix style\n\nbody here";
        assert_eq!(extract_header_value(msg, "Subject"), "Unix style");
    }

    #[test]
    fn extract_header_value_multiple_colons() {
        let msg = b"Subject: Re: Re: Important: urgent\r\n\r\n";
        assert_eq!(extract_header_value(msg, "Subject"), "Re: Re: Important: urgent");
    }

    #[test]
    fn extract_header_value_first_match_wins() {
        let msg = b"Subject: First\r\nSubject: Second\r\n\r\n";
        assert_eq!(extract_header_value(msg, "Subject"), "First");
    }

    #[test]
    fn extract_header_value_only_header_name_no_value() {
        // "Subject:" with nothing after => trim to empty
        let msg = b"Subject:\r\n\r\n";
        assert_eq!(extract_header_value(msg, "Subject"), "");
    }

    #[test]
    fn extract_header_value_utf8_content() {
        let msg = "Subject: 你好世界\r\n\r\nbody".as_bytes();
        assert_eq!(extract_header_value(msg, "Subject"), "你好世界");
    }

    #[test]
    fn extract_header_value_similar_prefix_no_match() {
        // "Subject-Alt" should not match "Subject"
        let msg = b"Subject-Alt: nope\r\nSubject: yes\r\n\r\n";
        assert_eq!(extract_header_value(msg, "Subject"), "yes");
    }

    // ---- FlagAction tests ----

    #[test]
    fn flag_action_clone_copy_eq() {
        let a = FlagAction::Set;
        let b = a;
        assert_eq!(a, b);
        assert_eq!(FlagAction::Add, FlagAction::Add);
        assert_ne!(FlagAction::Set, FlagAction::Remove);
    }

    #[test]
    fn flag_action_debug() {
        assert_eq!(format!("{:?}", FlagAction::Set), "Set");
        assert_eq!(format!("{:?}", FlagAction::Add), "Add");
        assert_eq!(format!("{:?}", FlagAction::Remove), "Remove");
    }

    // ---- EmailAnalysisRow tests ----

    #[test]
    fn email_analysis_row_clone_and_debug() {
        let row = crate::types::EmailAnalysisRow {
            message_id: 42,
            category: "finance".into(),
            risk_score: 75,
            risk_reason: "suspicious sender".into(),
            summary: "wire transfer request".into(),
            people: serde_json::json!(["Alice", "Bob"]),
            dates: serde_json::json!(["2026-03-01"]),
            amounts: serde_json::json!(["$1000"]),
            action_items: serde_json::json!(["review"]),
            model_version: "v2".into(),
            clean_text: "some cleaned text".into(),
            requires_action: true,
            sender_intent: "request".into(),
            action_deadline: Some("2026-03-15".into()),
        };
        let cloned = row.clone();
        assert_eq!(cloned.message_id, 42);
        assert_eq!(cloned.category, "finance");
        assert_eq!(cloned.risk_score, 75);
        assert_eq!(cloned.risk_reason, "suspicious sender");

        let debug = format!("{:?}", row);
        assert!(debug.contains("EmailAnalysisRow"));
        assert!(debug.contains("finance"));
    }

    // ---- normalize_email tests ----

    #[test]
    fn normalize_email_basic() {
        assert_eq!(normalize_email("Alice@Example.COM"), "alice@example.com");
    }

    #[test]
    fn normalize_email_with_display_name() {
        assert_eq!(normalize_email("Alice <alice@example.com>"), "alice@example.com");
        assert_eq!(normalize_email("\"Bob\" <BOB@Test.COM>"), "bob@test.com");
    }

    #[test]
    fn normalize_email_removes_plus_tag() {
        assert_eq!(normalize_email("user+tag@example.com"), "user@example.com");
        assert_eq!(normalize_email("alice+newsletter@test.com"), "alice@test.com");
    }

    #[test]
    fn normalize_email_no_plus_tag() {
        assert_eq!(normalize_email("alice@example.com"), "alice@example.com");
    }

    #[test]
    fn normalize_email_trims_whitespace() {
        assert_eq!(normalize_email("  alice@example.com  "), "alice@example.com");
    }

    #[test]
    fn normalize_email_bare_string() {
        assert_eq!(normalize_email("notanemail"), "notanemail");
    }
}
