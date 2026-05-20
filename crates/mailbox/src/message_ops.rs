use sqlx::PgPool;

use crate::helpers::{extract_header_value, row_to_message_meta};
use crate::store::MailboxStore;
use crate::threading;
use crate::types::MessageMeta;

impl MailboxStore {
    /// index a new message: assigns UID, inserts metadata, returns UID
    // 11 args mirror the messages-table columns the caller is already
    // computing from a parsed RFC 5322 message — wrapping them in a
    // struct adds a noisy round-trip without simplifying the call site.
    #[allow(clippy::too_many_arguments)]
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

    /// Batch-fetch `invite_method` for a list of message ids. Caller (the
    /// conversations API in MRS-18) drops these onto each
    /// ThreadMessageResponse so the web client can mount invite-card based
    /// on a server-authoritative signal rather than re-detecting via
    /// attachments. Skips rows where `invite_method IS NULL`.
    pub async fn get_invite_methods(
        &self,
        ids: &[i64],
    ) -> Result<Vec<(i64, String)>, sqlx::Error> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        sqlx::query_as::<_, (i64, String)>(
            "SELECT id, invite_method FROM messages
             WHERE id = ANY($1) AND invite_method IS NOT NULL",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await
    }

    /// Attach an iTIP invite payload (parsed by `mailrs::ical` upstream) to
    /// a previously-stored message. Idempotent: rerunning with new content
    /// overwrites. The caller (server inbound pipeline, MRS-4) extracts
    /// the `text/calendar` MIME part, parses it, and serialises the result
    /// to JSON before passing it here.
    ///
    /// Returns the `messages.id` of the updated row when the (user,
    /// folder, uid) tuple matches an existing message, `None` otherwise
    /// (e.g. when the message was moved or deleted between insertion and
    /// the post-store hook).
    pub async fn update_invite_payload(
        &self,
        user: &str,
        mailbox_name: &str,
        uid: u32,
        invite_payload: &serde_json::Value,
        invite_method: &str,
    ) -> Result<Option<i64>, sqlx::Error> {
        let row: Option<(i64,)> = sqlx::query_as(
            "UPDATE messages
             SET invite_payload = $1, invite_method = $2
             FROM mailboxes
             WHERE messages.mailbox_id = mailboxes.id
               AND mailboxes.user_address = $3
               AND mailboxes.name = $4
               AND messages.uid = $5
             RETURNING messages.id",
        )
        .bind(invite_payload)
        .bind(invite_method)
        .bind(user)
        .bind(mailbox_name)
        .bind(uid as i32)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id,)| id))
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

    /// get a message by its database primary key (for JMAP global emailId)
    pub async fn get_message_by_db_id(
        &self,
        user: &str,
        id: i64,
    ) -> Result<Option<MessageMeta>, sqlx::Error> {
        let row = sqlx::query_as::<_, (i64, i64, i32, String, String, String, String, i64, i32, i32, i64, String, String, String, i64)>(
            "SELECT m.id, m.mailbox_id, m.uid, m.maildir_id, m.sender, m.recipients, m.subject, m.date_epoch, m.size, m.flags, m.internal_date, m.message_id, m.in_reply_to, m.thread_id, m.modseq
             FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE m.id = $1 AND mb.user_address = $2",
        )
        .bind(id)
        .bind(user)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(row_to_message_meta))
    }

    /// query messages for JMAP: flexible filter returning DB IDs + total count
    // 8 args are independent filter axes (user/mailbox/text/has_flags/
    // not_flags/sort/limit/offset). A param struct would force callers
    // to construct + drop a builder for each call without making the
    // intent clearer than positional args.
    #[allow(clippy::too_many_arguments)]
    pub async fn query_messages(
        &self,
        user: &str,
        mailbox_id: Option<i64>,
        text: Option<&str>,
        has_flags: u32,
        not_flags: u32,
        sort_desc: bool,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<i64>, u32), sqlx::Error> {
        let mut conditions = vec!["mb.user_address = $1".to_string()];
        let mut param_idx = 2u32;

        let mut mailbox_bind = None;
        if let Some(mb_id) = mailbox_id {
            conditions.push(format!("m.mailbox_id = ${param_idx}"));
            mailbox_bind = Some(mb_id);
            param_idx += 1;
        }

        let mut text_bind = None;
        if let Some(t) = text
            && !t.is_empty() {
                conditions.push(format!(
                    "(m.search_vector @@ plainto_tsquery('simple', ${param_idx}) \
                     OR m.subject ILIKE ${param_idx} OR m.sender ILIKE ${param_idx})"
                ));
                text_bind = Some(format!("%{}%", t.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")));
                let _ = param_idx;
            }

        if has_flags != 0 {
            conditions.push(format!("(m.flags & {has_flags}) = {has_flags}"));
        }
        if not_flags != 0 {
            conditions.push(format!("(m.flags & {not_flags}) = 0"));
        }

        let where_clause = conditions.join(" AND ");
        let order = if sort_desc { "DESC" } else { "ASC" };

        // count total
        let count_sql = format!(
            "SELECT COUNT(*)::bigint FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id WHERE {where_clause}"
        );
        let mut count_q = sqlx::query_as::<_, (i64,)>(&count_sql).bind(user);
        if let Some(mb_id) = mailbox_bind {
            count_q = count_q.bind(mb_id);
        }
        if let Some(ref t) = text_bind {
            count_q = count_q.bind(t);
        }
        let total = count_q.fetch_one(&self.pool).await?.0 as u32;

        // fetch ids
        let ids_sql = format!(
            "SELECT m.id FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id \
             WHERE {where_clause} ORDER BY m.internal_date {order} LIMIT {limit} OFFSET {offset}"
        );
        let mut ids_q = sqlx::query_as::<_, (i64,)>(&ids_sql).bind(user);
        if let Some(mb_id) = mailbox_bind {
            ids_q = ids_q.bind(mb_id);
        }
        if let Some(ref t) = text_bind {
            ids_q = ids_q.bind(t);
        }
        let ids: Vec<i64> = ids_q.fetch_all(&self.pool).await?.into_iter().map(|(id,)| id).collect();

        Ok((ids, total))
    }

    /// delete messages with \Deleted flag, return expunged UIDs
    pub async fn expunge(&self, mailbox_id: i64) -> Result<Vec<u32>, sqlx::Error> {
        let uid_rows = sqlx::query_as::<_, (i32,)>(
            "SELECT uid FROM messages WHERE mailbox_id = $1 AND (flags & $2) != 0",
        )
        .bind(mailbox_id)
        .bind(crate::types::FLAG_DELETED as i32)
        .fetch_all(&self.pool)
        .await?;

        let uids: Vec<u32> = uid_rows.into_iter().map(|r| r.0 as u32).collect();

        sqlx::query("DELETE FROM messages WHERE mailbox_id = $1 AND (flags & $2) != 0")
            .bind(mailbox_id)
            .bind(crate::types::FLAG_DELETED as i32)
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
        let md = mailrs_maildir::Maildir::create(&path)
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

    /// count unread *threads* for the user, mirroring `list_conversations`
    /// thread-level aggregation so the dashboard tally matches what the
    /// inbox view shows. a thread counts when:
    ///   - it has at least one unseen message
    ///   - none of its messages are archived (matches list HAVING BOOL_OR)
    ///   - it isn't snoozed, isn't spam/scam, has a non-empty thread_id
    ///   - the newest message's sender isn't the user (same "don't show
    ///     my own outbox in All" filter list_conversations applies)
    ///
    /// the function name keeps "unseen" for back-compat but the count is
    /// thread-level — `unread_messages` in the API was always displayed as
    /// "Unread N" without specifying messages vs threads, and threads are
    /// what the user actually sees in the list
    pub async fn count_unseen(&self, user: &str) -> i64 {
        let row: Result<(i64,), _> = sqlx::query_as(
            "SELECT COUNT(*) FROM (
               SELECT m.thread_id
               FROM messages m
               JOIN mailboxes mb ON m.mailbox_id = mb.id
               WHERE mb.user_address = $1
                 AND m.thread_id != ''
                 AND NOT EXISTS (
                   SELECT 1 FROM snoozed_conversations sc
                   WHERE sc.thread_id = m.thread_id
                     AND sc.account_address = mb.user_address
                     AND sc.snoozed_until > NOW()
                 )
                 AND NOT EXISTS (
                   SELECT 1 FROM email_analysis ea
                   WHERE ea.message_id = m.id AND ea.category IN ('spam', 'scam')
                 )
               GROUP BY m.thread_id
               HAVING BOOL_OR(m.archived) = false
                  AND COUNT(*) FILTER (WHERE (m.flags & 1) = 0) > 0
                  AND LOWER(COALESCE((SELECT m_last.sender FROM messages m_last WHERE m_last.thread_id = m.thread_id ORDER BY m_last.internal_date DESC LIMIT 1), '')) NOT LIKE '%' || LOWER($1) || '%'
             ) t",
        )
        .bind(user)
        .fetch_one(&self.pool)
        .await;

        row.map(|r| r.0).unwrap_or(0)
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

    /// find a message by its uid (across all user's mailboxes)
    pub async fn find_message_by_uid(
        &self,
        user: &str,
        uid: u32,
    ) -> Result<Option<MessageMeta>, sqlx::Error> {
        let row = sqlx::query_as::<_, (i64, i64, i32, String, String, String, String, i64, i32, i32, i64, String, String, String, i64)>(
            "SELECT m.id, m.mailbox_id, m.uid, m.maildir_id, m.sender, m.recipients,
                    m.subject, m.date_epoch, m.size, m.flags, m.internal_date, m.message_id,
                    m.in_reply_to, m.thread_id, m.modseq
             FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1 AND m.uid = $2
             LIMIT 1",
        )
        .bind(user)
        .bind(uid as i32)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(row_to_message_meta))
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

    /// update message content fields after deep cleaning
    // 7 of the args are Option<&str> columns being patched; positional is
    // the cleanest spelling — wrapping in a struct of `Option<&str>` adds
    // no clarity over the column order they already mirror.
    #[allow(clippy::too_many_arguments)]
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

    /// set the BIMI logo URL on a message
    pub async fn update_bimi_logo(
        &self,
        message_id: i64,
        logo_url: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE messages SET bimi_logo_url = $2 WHERE id = $1")
            .bind(message_id)
            .bind(logo_url)
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
