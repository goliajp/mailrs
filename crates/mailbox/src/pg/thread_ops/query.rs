//! Conversation-list queries (read-only).
//!
//! The two big SELECTs that build conversation summaries from
//! grouped messages — both share the same projection shape and are
//! used by every "give me the inbox" path.

use crate::pg::PgMailboxStore;
use crate::types::ConversationSummary;

impl PgMailboxStore {
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
}
