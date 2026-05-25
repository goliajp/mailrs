use crate::pg::PgMailboxStore;
use crate::pg::helpers::{read_raw_from_maildir, row_to_message_meta_from_row};
use crate::threading;
use crate::types::{ConversationSummary, FLAG_FLAGGED, FLAG_SEEN, MessageMeta};

impl PgMailboxStore {
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
    // 11 args are independent filter axes for the conversation list
    // (user/limit/before_ts + 8 filters). Wrapping in a single
    // `ConversationFilter` struct is on the v2 roadmap; for now this
    // matches the JMAP query shape callers already use.
    #[allow(clippy::too_many_arguments)]
    pub async fn list_conversations(
        &self,
        user: &str,
        limit: u32,
        before_ts: Option<i64>,
        category: Option<&str>,
        domains: Option<&[String]>,
        archived: bool,
        folder: Option<&str>,
        unread: Option<bool>,
        starred: Option<bool>,
        section: Option<&str>,
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
                let placeholders: Vec<String> = doms
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("${}", param_idx + i as u32))
                    .collect();
                param_idx += doms.len() as u32;
                format!(
                    "mb.user_address IN (SELECT address FROM accounts WHERE domain IN ({}))",
                    placeholders.join(",")
                )
            }
        } else {
            param_idx += 1;
            format!("mb.user_address = ${}", param_idx - 1)
        };
        conditions.insert(0, user_condition);

        // exclude snoozed conversations (snooze still active)
        conditions.push(
            "NOT EXISTS (SELECT 1 FROM snoozed_conversations sc WHERE sc.thread_id = m.thread_id AND sc.account_address = mb.user_address AND sc.snoozed_until > NOW())".to_string()
        );

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
        // perf: SubPlan 5 + 8 (per-row lookups on email_analysis for
        // requires_action and spam/scam exclusion) collapsed into a single
        // LEFT JOIN. one merge/hash join instead of ~36k index probes
        // per request (perfs/topics/01 fix-c).
        if category.is_some() {
            conditions.push(format!("ea.category = ${param_idx}"));
        } else {
            // exclude spam/scam from default view — users must select the category explicitly
            conditions.push("COALESCE(ea.category, 'general') NOT IN ('spam', 'scam')".to_string());
        }

        let where_clause = conditions.join(" AND ");

        // build HAVING clause with optional filters
        let mut having_parts = vec![archived_filter.to_string()];
        // For "All" / no-folder view, only include threads that have at
        // least one message NOT in the Sent mailbox. This keeps Sent-only
        // threads (drafts the user dispatched without any reply) out of the
        // default reading list, while still letting threads with both
        // inbound and outbound messages appear in BOTH All AND Sent —
        // exactly what the user expects of a conversation grouping.
        if folder.is_none() {
            having_parts.push("BOOL_OR(mb.name != 'Sent') = true".to_string());
        }
        if unread == Some(true) {
            having_parts.push(format!("{unread_expr} > 0"));
        }
        if starred == Some(true) {
            having_parts.push("BOOL_OR((m.flags & 4) != 0) = true".to_string());
        }
        match section {
            // perf: ea.requires_action comes from the LEFT JOIN above
            Some("action") => having_parts
                .push("COALESCE(BOOL_OR(ea.requires_action), false) = true".to_string()),
            // perf: ordered aggregate replaces a per-group SubPlan that ran
            // a LIMIT-1 query on messages for each thread (perfs/topics/07).
            // matches the SELECT-list expression so PG computes it once.
            Some("important") => having_parts.push(
                "COALESCE((array_agg(m.importance_level ORDER BY m.importance_score DESC NULLS LAST))[1], 'normal') IN ('critical', 'important')".to_string()
            ),
            Some("other") => having_parts.push(
                "COALESCE((array_agg(m.importance_level ORDER BY m.importance_score DESC NULLS LAST))[1], 'normal') IN ('low', 'noise')".to_string()
            ),
            _ => {}
        }
        let having_clause = having_parts.join(" AND ");

        let sql = format!(
            "SELECT m.thread_id, MAX(m.subject), string_agg(DISTINCT m.sender, ','),
                    {count_expr}, {unread_expr}, MAX(m.internal_date),
                    COALESCE((SELECT ea.category FROM email_analysis ea
                              JOIN messages m2 ON ea.message_id = m2.id
                              WHERE m2.thread_id = m.thread_id
                              ORDER BY m2.internal_date DESC LIMIT 1), 'general'),
                    BOOL_OR((m.flags & 4) != 0),
                    COALESCE(
                      (SELECT ea_snip.summary FROM email_analysis ea_snip
                       JOIN messages m_snip ON ea_snip.message_id = m_snip.id
                       WHERE m_snip.thread_id = m.thread_id AND ea_snip.summary IS NOT NULL AND ea_snip.summary != ''
                       ORDER BY m_snip.internal_date DESC LIMIT 1),
                      (SELECT LEFT(m3.text_body, 120) FROM messages m3
                       WHERE m3.thread_id = m.thread_id AND m3.text_body IS NOT NULL AND m3.text_body != ''
                       ORDER BY m3.internal_date DESC LIMIT 1),
                      ''),
                    BOOL_OR(m.pinned),
                    BOOL_OR(m.archived),
                    COALESCE((array_agg(m.importance_level ORDER BY m.importance_score DESC NULLS LAST))[1], 'normal'),
                    COALESCE(MAX(m.importance_score), 0.0),
                    COALESCE(BOOL_OR(ea.requires_action), false),
                    COALESCE((array_agg(m.sender ORDER BY m.internal_date DESC))[1], ''),
                    COUNT(DISTINCT CASE WHEN mb.name  = 'Sent' AND m.message_id != '' THEN m.message_id WHEN mb.name  = 'Sent' THEN CAST(m.id AS TEXT) END)
             FROM messages m
                  JOIN mailboxes mb ON m.mailbox_id = mb.id
                  LEFT JOIN email_analysis ea ON ea.message_id = m.id
             WHERE {where_clause}
             GROUP BY m.thread_id HAVING {having_clause}
             ORDER BY BOOL_OR(m.pinned) DESC, MAX(m.internal_date) DESC LIMIT ${limit_idx}"
        );

        // bind parameters in order
        let mut query = sqlx::query_as::<
            _,
            (
                String,
                Option<String>,
                Option<String>,
                i64,
                i64,
                i64,
                String,
                bool,
                String,
                bool,
                bool,
                String,
                f32,
                bool,
                String,
                i64,
            ),
        >(&sql);

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
                requires_action: r.13,
                last_sender: r.14,
                sent_count: r.15 as u32,
            })
            .collect())
    }

    /// fetch conversation summaries for specific thread_ids (used by meilisearch integration)
    pub async fn get_conversations_by_thread_ids(
        &self,
        user: &str,
        thread_ids: &[String],
        domains: Option<&[String]>,
    ) -> Result<Vec<ConversationSummary>, sqlx::Error> {
        if thread_ids.is_empty() {
            return Ok(Vec::new());
        }

        let count_expr = "COUNT(DISTINCT CASE WHEN m.message_id != '' THEN m.message_id ELSE CAST(m.id AS TEXT) END)";
        let unread_expr = "COUNT(DISTINCT CASE WHEN (m.flags & 1) = 0 THEN CASE WHEN m.message_id != '' THEN m.message_id ELSE CAST(m.id AS TEXT) END END)";

        // build user filter
        let mut param_idx = 1u32;
        let user_condition = if let Some(doms) = domains {
            if doms.is_empty() {
                param_idx += 1;
                format!("mb.user_address = ${}", param_idx - 1)
            } else {
                let placeholders: Vec<String> = doms
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("${}", param_idx + i as u32))
                    .collect();
                param_idx += doms.len() as u32;
                format!(
                    "mb.user_address IN (SELECT address FROM accounts WHERE domain IN ({}))",
                    placeholders.join(",")
                )
            }
        } else {
            param_idx += 1;
            format!("mb.user_address = ${}", param_idx - 1)
        };

        // build thread_id IN clause
        let tid_placeholders: Vec<String> = thread_ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("${}", param_idx + i as u32))
            .collect();
        let tid_filter = format!("m.thread_id IN ({})", tid_placeholders.join(","));

        let sql = format!(
            "SELECT m.thread_id, MAX(m.subject), string_agg(DISTINCT m.sender, ','),
                    {count_expr}, {unread_expr}, MAX(m.internal_date),
                    COALESCE((SELECT ea.category FROM email_analysis ea
                              JOIN messages m2 ON ea.message_id = m2.id
                              WHERE m2.thread_id = m.thread_id
                              ORDER BY m2.internal_date DESC LIMIT 1), 'general'),
                    BOOL_OR((m.flags & 4) != 0),
                    COALESCE(
                      (SELECT ea_snip.summary FROM email_analysis ea_snip
                       JOIN messages m_snip ON ea_snip.message_id = m_snip.id
                       WHERE m_snip.thread_id = m.thread_id AND ea_snip.summary IS NOT NULL AND ea_snip.summary != ''
                       ORDER BY m_snip.internal_date DESC LIMIT 1),
                      (SELECT LEFT(m3.text_body, 120) FROM messages m3
                       WHERE m3.thread_id = m.thread_id AND m3.text_body IS NOT NULL AND m3.text_body != ''
                       ORDER BY m3.internal_date DESC LIMIT 1),
                      ''),
                    BOOL_OR(m.pinned),
                    BOOL_OR(m.archived),
                    COALESCE((array_agg(m.importance_level ORDER BY m.importance_score DESC NULLS LAST))[1], 'normal'),
                    COALESCE(MAX(m.importance_score), 0.0),
                    COALESCE(BOOL_OR((SELECT ea_act.requires_action FROM email_analysis ea_act WHERE ea_act.message_id = m.id)), false),
                    COALESCE((SELECT m_last.sender FROM messages m_last WHERE m_last.thread_id = m.thread_id ORDER BY m_last.internal_date DESC LIMIT 1), ''),
                    COUNT(DISTINCT CASE WHEN mb.name  = 'Sent' AND m.message_id != '' THEN m.message_id WHEN mb.name  = 'Sent' THEN CAST(m.id AS TEXT) END)
             FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE {user_condition} AND {tid_filter}
             GROUP BY m.thread_id"
        );

        let mut query = sqlx::query_as::<
            _,
            (
                String,
                Option<String>,
                Option<String>,
                i64,
                i64,
                i64,
                String,
                bool,
                String,
                bool,
                bool,
                String,
                f32,
                bool,
                String,
                i64,
            ),
        >(&sql);

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

        for tid in thread_ids {
            query = query.bind(tid);
        }

        let rows = query.fetch_all(&self.pool).await?;

        // preserve the order from thread_ids (meilisearch relevance order)
        let map: std::collections::HashMap<String, ConversationSummary> = rows
            .into_iter()
            .map(|r| {
                let tid = r.0.clone();
                let cs = ConversationSummary {
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
                    requires_action: r.13,
                    last_sender: r.14,
                    sent_count: r.15 as u32,
                };
                (tid, cs)
            })
            .collect();

        Ok(thread_ids
            .iter()
            .filter_map(|tid| map.get(tid).cloned())
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
                let placeholders: Vec<String> = doms
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("${}", i + 3))
                    .collect();
                let f = format!(
                    "mb.user_address IN (SELECT address FROM accounts WHERE domain IN ({}))",
                    placeholders.join(",")
                );
                let f2 = format!(
                    "mb2.user_address IN (SELECT address FROM accounts WHERE domain IN ({}))",
                    placeholders.join(",")
                );
                (f, f2)
            } else {
                (
                    "mb.user_address = $1".to_string(),
                    "mb2.user_address = $1".to_string(),
                )
            }
        } else {
            (
                "mb.user_address = $1".to_string(),
                "mb2.user_address = $1".to_string(),
            )
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

        let mut query = sqlx::query(&sql).bind(user).bind(thread_id);

        if let Some(doms) = domains
            && !doms.is_empty()
        {
            for d in doms {
                query = query.bind(d);
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
        let (user_filter, extra_params) = if let Some(doms) = domains.filter(|d| !d.is_empty()) {
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
    pub async fn unsnooze_thread(&self, user: &str, thread_id: &str) -> Result<(), sqlx::Error> {
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

    /// dismiss action for all messages in a thread: clear requires_action and reverse importance boost
    pub async fn dismiss_thread_action(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<u32, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE email_analysis SET requires_action = false
             WHERE message_id IN (
               SELECT m.id FROM messages m
               JOIN mailboxes mb ON m.mailbox_id = mb.id
               WHERE m.thread_id = $1 AND mb.user_address = $2
             ) AND requires_action = true",
        )
        .bind(thread_id)
        .bind(user)
        .execute(&self.pool)
        .await?;

        let affected = result.rows_affected() as u32;

        if affected > 0 {
            sqlx::query(
                "UPDATE messages SET
                   importance_score = GREATEST(-0.5, importance_score - 0.2),
                   importance_level = CASE
                     WHEN GREATEST(-0.5, importance_score - 0.2) >= 0.8 THEN 'critical'
                     WHEN GREATEST(-0.5, importance_score - 0.2) >= 0.5 THEN 'important'
                     WHEN GREATEST(-0.5, importance_score - 0.2) >= 0.2 THEN 'normal'
                     WHEN GREATEST(-0.5, importance_score - 0.2) >= 0.0 THEN 'low'
                     ELSE 'noise'
                   END
                 WHERE thread_id = $1
                   AND mailbox_id IN (SELECT id FROM mailboxes WHERE user_address = $2)",
            )
            .bind(thread_id)
            .bind(user)
            .execute(&self.pool)
            .await?;
        }

        Ok(affected)
    }
}
