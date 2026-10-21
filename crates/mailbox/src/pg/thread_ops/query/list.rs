//! `list_conversations` — the inbox, and every filtered view of it.

use crate::pg::PgMailboxStore;
use crate::types::ConversationSummary;

mod clauses;

use clauses::Filters;

type ListRow = (
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
);

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
        let list = Filters {
            before_ts,
            category,
            domains,
            archived,
            folder,
            unread,
            starred,
            section,
        }
        .query();

        // bind parameters in order
        let mut query = sqlx::query_as::<_, ListRow>(&list.sql);

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

        // Only Sent / legacy mailbox-name folders bind $folder; the
        // triage buckets filter via HAVING literals (no bound param).
        if let Some(f) = folder
            && list.folder_binds_name
        {
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

        Ok(rows.into_iter().map(summary_from_row).collect())
    }
}

fn summary_from_row(r: ListRow) -> ConversationSummary {
    ConversationSummary {
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
    }
}
