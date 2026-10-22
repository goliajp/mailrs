//! Read-only message lookups and queries.

use crate::pg::PgMailboxStore;
use crate::pg::helpers::row_to_message_meta;
use crate::types::MessageMeta;

impl PgMailboxStore {
    /// List messages in a mailbox ordered by UID, with offset/limit
    /// pagination.
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

    /// Fetch a single message by mailbox + IMAP UID. Returns `None` when
    /// no such message exists.
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
            && !t.is_empty()
        {
            // PG full-text search — the only search path.
            // tsvector branch restored in the D-pre revert — the engine
            // covers tsvector since SPG round-10.
            conditions.push(format!(
                "(m.search_vector @@ plainto_tsquery('simple', ${param_idx}) \
                     OR m.subject ILIKE ${param_idx} OR m.sender ILIKE ${param_idx})"
            ));
            text_bind = Some(format!(
                "%{}%",
                t.replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            ));
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
        let ids: Vec<i64> = ids_q
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|(id,)| id)
            .collect();

        Ok((ids, total))
    }

    /// find a message by its message_id header (across all user's mailboxes)
    pub async fn find_message_by_message_id(
        &self,
        user: &str,
        message_id: &str,
    ) -> Result<Option<MessageMeta>, sqlx::Error> {
        let row = sqlx::query_as::<
            _,
            (
                i64,
                i64,
                i32,
                String,
                String,
                String,
                String,
                i64,
                i32,
                i32,
                i64,
                String,
                String,
                String,
                i64,
            ),
        >(
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
        let row = sqlx::query_as::<
            _,
            (
                i64,
                i64,
                i32,
                String,
                String,
                String,
                String,
                i64,
                i32,
                i32,
                i64,
                String,
                String,
                String,
                i64,
            ),
        >(
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
}
