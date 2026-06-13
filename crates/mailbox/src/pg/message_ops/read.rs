//! Read-only message lookups and queries.

use crate::pg::PgMailboxStore;
use crate::pg::helpers::row_to_message_meta;
use crate::types::MessageMeta;

impl PgMailboxStore {
    /// Batch-fetch `invite_method` for a list of message ids. Caller (the
    /// conversations API in MRS-18) drops these onto each
    /// ThreadMessageResponse so the web client can mount invite-card based
    /// on a server-authoritative signal rather than re-detecting via
    /// attachments. Skips rows where `invite_method IS NULL`.
    pub async fn get_invite_methods(&self, ids: &[i64]) -> Result<Vec<(i64, String)>, sqlx::Error> {
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
            && !t.is_empty()
        {
            // PG search is the fallback path (main is Meilisearch).
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
    pub async fn count_unseen(&self, user: &str) -> Result<i64, sqlx::Error> {
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

        // propagate the error rather than swallow to 0 (a stone doesn't
        // log; the caller decides). swallowing here hid an engine
        // FILTER-clause parse failure for weeks — the homepage showed 0
        // unread on otherwise-full mailboxes (incident 2026-06-13).
        row.map(|r| r.0)
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
