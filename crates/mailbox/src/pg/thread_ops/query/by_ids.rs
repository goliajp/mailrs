//! `get_conversations_by_thread_ids` — hydrating search hits into rows.
//!
//! Same projection as the list, driven by an explicit id set instead of the
//! filter axes. The duplication is deliberate for now: folding them together
//! means one function with both the filter machinery and the id path, and the
//! projection is what would have to stay in step either way.

use crate::pg::PgMailboxStore;
use crate::types::ConversationSummary;

impl PgMailboxStore {
    /// fetch conversation summaries for specific thread_ids (used to hydrate search hits)
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
                      (SELECT LEFT(ea_snip.summary, 80) FROM email_analysis ea_snip
                       JOIN messages m_snip ON ea_snip.message_id = m_snip.id
                       WHERE m_snip.thread_id = m.thread_id AND ea_snip.summary IS NOT NULL AND ea_snip.summary != ''
                       ORDER BY m_snip.internal_date DESC LIMIT 1),
                      (SELECT LEFT(m3.text_body, 80) FROM messages m3
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

        // preserve the order from thread_ids (search relevance order)
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
                    // The dormant pg lane has no per-user snooze column;
                    // snoozing is served by the kevy lane production runs.
                    snoozed_until: 0,
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
