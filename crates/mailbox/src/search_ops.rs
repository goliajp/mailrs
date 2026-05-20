use crate::helpers::build_user_filter;
use crate::store::MailboxStore;
use crate::types::ConversationSummary;

impl MailboxStore {
    /// search conversations by subject or sender (ILIKE search)
    pub async fn search_conversations(
        &self,
        user: &str,
        query: &str,
        limit: u32,
        category: Option<&str>,
        domains: Option<&[String]>,
    ) -> Result<Vec<ConversationSummary>, sqlx::Error> {
        let count_expr = "COUNT(DISTINCT CASE WHEN m.message_id != '' THEN m.message_id ELSE CAST(m.id AS TEXT) END)";
        let unread_expr = "COUNT(DISTINCT CASE WHEN (m.flags & 1) = 0 THEN CASE WHEN m.message_id != '' THEN m.message_id ELSE CAST(m.id AS TEXT) END END)";

        let mut param_idx = 1u32;

        let (user_filter, user_binds) = build_user_filter(user, domains, param_idx);
        param_idx += user_binds.len() as u32;

        // two query params: raw query for tsvector, %pattern% for ILIKE
        let tsquery_idx = param_idx;
        param_idx += 1;
        let pattern_idx = param_idx;
        param_idx += 1;
        let limit_idx = param_idx;
        param_idx += 1;

        let category_filter = if category.is_some() {
            format!("AND ea.category = ${param_idx}")
        } else {
            String::new()
        };

        // perf (perfs/topic-06): the previous shape was one big WHERE-clause
        // OR-chain across 5 ILIKE columns + tsvector + EXISTS. PG can't
        // combine that into a BitmapOr so it fell back to scanning every
        // row of the user's messages — 575 ms on the lihao@golia.jp mailbox.
        //
        // two changes make it use every available index:
        //   1. a CTE with one branch per column, joined by UNION. each
        //      branch matches a single index (idx_messages_search_vector
        //      gin tsvector, idx_messages_*_trgm gin trigram, the
        //      attachment_content seq scan).
        //   2. each ILIKE branch repeats the partial index's WHERE
        //      condition (`subject IS NOT NULL AND subject != ''` etc.)
        //      so PG can prove the row qualifies for the partial index
        //      and uses Bitmap Index Scan instead of Seq Scan.
        // result on prod: 575 ms -> 45 ms (-92%) for q=invoice.
        // hit any message in a thread → surface the whole thread. The first
        // CTE finds the matching message ids; the second walks those threads
        // back out so the GROUP BY at the bottom of the query sees every
        // message in the thread (not just the matched one). Without this,
        // search results showed a thread with message_count=1 / snippet from
        // only the matched message and the user lost the conversation context.
        let search_filter = format!(
            "WITH matched AS (
               SELECT id FROM messages WHERE search_vector @@ plainto_tsquery('simple', ${tsquery_idx})
               UNION SELECT id FROM messages WHERE subject IS NOT NULL AND subject != '' AND subject ILIKE ${pattern_idx}
               UNION SELECT id FROM messages WHERE sender IS NOT NULL AND sender != '' AND sender ILIKE ${pattern_idx}
               UNION SELECT id FROM messages WHERE recipients IS NOT NULL AND recipients != '' AND recipients ILIKE ${pattern_idx}
               UNION SELECT id FROM messages WHERE text_body IS NOT NULL AND text_body != '' AND text_body ILIKE ${pattern_idx}
               UNION SELECT id FROM messages WHERE clean_text IS NOT NULL AND clean_text != '' AND clean_text ILIKE ${pattern_idx}
               UNION SELECT message_id FROM attachment_content WHERE extracted_text ILIKE ${pattern_idx}
             ),
             cands AS (
               SELECT m_all.id
                 FROM messages m_all
                WHERE m_all.thread_id IN (SELECT thread_id FROM messages WHERE id IN (SELECT id FROM matched))
             )"
        );

        // order by relevance (ts_rank) when tsvector matches, else by date
        let order_expr = format!(
            "MAX(CASE WHEN m.search_vector @@ plainto_tsquery('simple', ${tsquery_idx}) \
             THEN ts_rank(m.search_vector, plainto_tsquery('simple', ${tsquery_idx})) ELSE 0 END) DESC, \
             MAX(m.internal_date) DESC"
        );

        let sql = format!(
            "{search_filter}
             SELECT m.thread_id, MAX(m.subject), string_agg(DISTINCT m.sender, ','),
                    {count_expr}, {unread_expr}, MAX(m.internal_date),
                    COALESCE((SELECT ea2.category FROM email_analysis ea2
                              JOIN messages m2 ON ea2.message_id = m2.id
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
                  JOIN cands ON cands.id = m.id
                  JOIN mailboxes mb ON m.mailbox_id = mb.id
                  LEFT JOIN email_analysis ea ON ea.message_id = m.id
             WHERE {user_filter} AND thread_id != ''
               {category_filter}
             GROUP BY m.thread_id HAVING BOOL_OR(m.archived) = false
             ORDER BY {order_expr} LIMIT ${limit_idx}"
        );

        // for ILIKE, wrap query with %
        let escaped = query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        let pattern = format!("%{escaped}%");

        let mut q = sqlx::query_as::<_, (String, Option<String>, Option<String>, i64, i64, i64, String, bool, String, bool, bool, String, f32, bool, String, i64)>(&sql);

        for b in &user_binds {
            q = q.bind(b);
        }

        q = q.bind(query).bind(&pattern).bind(limit as i64);

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
                requires_action: r.13,
                last_sender: r.14,
                sent_count: r.15 as u32,
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

    /// count threads with requires_action = true (not archived)
    pub async fn count_action_threads(
        &self,
        user: &str,
        domains: Option<&[String]>,
    ) -> Result<i64, sqlx::Error> {
        let (user_filter, binds) = build_user_filter(user, domains, 1);

        let sql = format!(
            "SELECT COUNT(DISTINCT m.thread_id)
             FROM email_analysis ea
             JOIN messages m ON ea.message_id = m.id
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE {user_filter} AND ea.requires_action = true
               AND NOT EXISTS (SELECT 1 FROM email_analysis ea_ex WHERE ea_ex.message_id = m.id AND ea_ex.category IN ('spam', 'scam'))
               AND COALESCE(m.archived, false) = false"
        );

        let mut query = sqlx::query_as::<_, (i64,)>(&sql);
        for b in &binds {
            query = query.bind(b);
        }

        let (count,) = query.fetch_one(&self.pool).await?;
        Ok(count)
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
}
