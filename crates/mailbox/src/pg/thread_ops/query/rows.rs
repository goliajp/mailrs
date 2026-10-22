//! One row per (user, thread): what every conversation view reads.
//!
//! The kevy store keeps a membership row per (user, thread) and every list,
//! count and search reads that row. This is the same row, aggregated from a
//! user's messages, so the views here read one definition too: the list,
//! the rows by id, search hydration, the category counts and the unread
//! count cannot each derive "unread" or "sent-only" their own way.
//!
//! Which rows a view wants is a [`Scope`] plus flags, the scope shared with
//! the kevy store; [`scope_predicate`] is its translation into SQL, tested
//! against [`Scope::admits`].

use crate::pg::PgMailboxStore;
use crate::scope::{Bucket, CATEGORY_BUCKETS, Scope};
use crate::types::ConversationSummary;

/// Which of a user's conversations to read.
#[derive(Debug, Clone)]
pub struct ThreadQuery<'a> {
    /// Whose conversations.
    pub user: &'a str,
    /// The folder and category the view is scoped to.
    pub scope: Scope,
    /// Only threads the user wrote into (the Sent view).
    pub sent: bool,
    /// Archived threads (`Some(true)`), live ones (`Some(false)`), or both.
    pub archived: Option<bool>,
    /// Only threads with unread mail.
    pub unread: bool,
    /// Only starred threads.
    pub starred: bool,
    /// Held threads: hidden, shown alongside the rest, or the only ones.
    pub held: Held,
    /// Only these threads, whatever their scope.
    pub thread_ids: Option<&'a [String]>,
    /// Only threads whose last date is before this.
    pub before_ts: Option<i64>,
    /// At most this many rows.
    pub limit: Option<i64>,
}

/// How a view treats conversations held for review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Held {
    /// Out of the view, as in every ordinary list.
    Hide,
    /// In the view with the rest.
    Include,
    /// The view is the held conversations.
    Only,
}

impl<'a> ThreadQuery<'a> {
    /// Every live, unheld conversation `user` has.
    pub fn all(user: &'a str) -> Self {
        Self {
            user,
            scope: Scope::All,
            sent: false,
            archived: Some(false),
            unread: false,
            starred: false,
            held: Held::Hide,
            thread_ids: None,
            before_ts: None,
            limit: None,
        }
    }
}

/// A row as the query returns it.
#[derive(sqlx::FromRow)]
struct Row {
    thread_id: String,
    subject: String,
    participants: String,
    message_count: i64,
    unread_count: i64,
    last_date: i64,
    category: String,
    flagged: bool,
    snippet: String,
    pinned: bool,
    archived: bool,
    importance_level: String,
    importance_score: f32,
    requires_action: bool,
    last_sender: String,
    sent_count: i64,
    snoozed_until: i64,
    quarantined: bool,
}

/// A message counted once even when it sits in two mailboxes.
const MESSAGE_KEY: &str =
    "CASE WHEN m.message_id != '' THEN m.message_id ELSE CAST(m.id AS TEXT) END";

/// The per-thread row for the user bound as `$1`.
///
/// The category is the newest classified message's, so an arrival that was
/// classified moves the thread, as it does in the kevy store; a thread with
/// nothing classified is Junk when its mail was filed there and Inbox
/// otherwise. Dates, the preview and the sender follow inbound mail: the
/// user's own reply does not re-date or re-preview the row.
fn thread_rows_sql(ids_param: Option<usize>) -> String {
    let ids = ids_param
        .map(|p| format!(" AND m.thread_id = ANY(${p})"))
        .unwrap_or_default();
    format!(
        "SELECT m.thread_id AS thread_id,
                COALESCE(MAX(m.subject), '') AS subject,
                COALESCE(string_agg(m.sender, ',' ORDER BY m.internal_date, m.id), '') AS participants,
                COUNT(DISTINCT {MESSAGE_KEY}) AS message_count,
                COUNT(DISTINCT CASE WHEN (m.flags & 1) = 0 THEN {MESSAGE_KEY} END) AS unread_count,
                COALESCE(MAX(CASE WHEN mb.name != 'Sent' THEN m.internal_date END), MAX(m.internal_date)) AS last_date,
                COALESCE((array_agg(ea.category ORDER BY (ea.category IS NULL), m.internal_date DESC))[1],
                         CASE WHEN BOOL_OR(mb.name = 'Junk') THEN 'spam' ELSE 'inbox' END) AS category,
                BOOL_OR((m.flags & 4) != 0) AS flagged,
                COALESCE((array_agg(m.snippet ORDER BY (m.snippet = ''), (mb.name = 'Sent'), m.internal_date DESC))[1], '') AS snippet,
                BOOL_OR(m.pinned) AS pinned,
                BOOL_OR(m.archived) AS archived,
                COALESCE((array_agg(m.importance_level ORDER BY m.importance_score DESC))[1], '') AS importance_level,
                COALESCE(MAX(m.importance_score), 0.0) AS importance_score,
                COALESCE(BOOL_OR(ea.requires_action), false) AS requires_action,
                COALESCE((array_agg(m.sender ORDER BY (mb.name = 'Sent'), m.internal_date DESC))[1], '') AS last_sender,
                COUNT(DISTINCT CASE WHEN mb.name = 'Sent' THEN {MESSAGE_KEY} END) AS sent_count,
                BOOL_AND(mb.name = 'Sent') AS sent_only,
                COALESCE(MAX(CAST(EXTRACT(EPOCH FROM sc.snoozed_until) AS BIGINT)), 0) AS snoozed_until,
                BOOL_OR(q.thread_id IS NOT NULL) AS quarantined
           FROM messages m
                JOIN mailboxes mb ON m.mailbox_id = mb.id
                LEFT JOIN email_analysis ea ON ea.message_id = m.id
                LEFT JOIN snoozed_conversations sc
                       ON sc.thread_id = m.thread_id AND sc.account_address = mb.user_address
                LEFT JOIN quarantine_holds q
                       ON q.thread_id = m.thread_id AND q.account_address = mb.user_address
          WHERE mb.user_address = $1 AND m.thread_id != ''{ids}
          GROUP BY m.thread_id"
    )
}

/// The bucket of a row's category, in SQL, from the same table
/// [`crate::scope::bucket_of`] reads.
fn bucket_sql() -> String {
    let mut arms = String::from("CASE");
    for b in [Bucket::Junk, Bucket::Notifications, Bucket::Promotions] {
        let names: Vec<String> = CATEGORY_BUCKETS
            .iter()
            .filter(|(_, x)| *x == b)
            .map(|(n, _)| format!("'{n}'"))
            .collect();
        arms.push_str(&format!(
            " WHEN LOWER(TRIM(t.category)) IN ({}) THEN '{}'",
            names.join(","),
            b.name()
        ));
    }
    arms.push_str(" ELSE 'inbox' END");
    arms
}

/// `scope` as a predicate over a row `t`. A category comes back as a bind
/// value for `$category_param`, never spliced into the text.
pub(crate) fn scope_predicate(scope: &Scope, category_param: usize) -> (String, Option<String>) {
    let bucket = bucket_sql();
    let in_bucket = |b: Bucket| {
        let sent_only = match b {
            Bucket::Inbox => " AND NOT t.sent_only",
            _ => "",
        };
        format!("({bucket} = '{}'{sent_only})", b.name())
    };
    match scope {
        Scope::All => ("TRUE".into(), None),
        Scope::NotJunk => (
            format!(
                "({} OR {} OR {})",
                in_bucket(Bucket::Inbox),
                in_bucket(Bucket::Notifications),
                in_bucket(Bucket::Promotions)
            ),
            None,
        ),
        Scope::Bucket(b) => (in_bucket(*b), None),
        Scope::Np => (
            format!(
                "({} OR {})",
                in_bucket(Bucket::Notifications),
                in_bucket(Bucket::Promotions)
            ),
            None,
        ),
        Scope::Category(c) => (
            format!("LOWER(t.category) = LOWER(${category_param})"),
            Some(c.clone()),
        ),
        Scope::Contradiction => ("FALSE".into(), None),
    }
}

impl From<Row> for ConversationSummary {
    fn from(r: Row) -> Self {
        ConversationSummary {
            thread_id: r.thread_id,
            subject: r.subject,
            participants: crate::participants::union([r.participants.as_str()]),
            message_count: r.message_count as u32,
            unread_count: r.unread_count as u32,
            last_date: r.last_date,
            category: r.category,
            flagged: r.flagged,
            snippet: r.snippet,
            pinned: r.pinned,
            archived: r.archived,
            importance_level: r.importance_level,
            importance_score: r.importance_score,
            requires_action: r.requires_action,
            last_sender: r.last_sender,
            sent_count: r.sent_count as u32,
            snoozed_until: r.snoozed_until,
            quarantined: r.quarantined,
        }
    }
}

impl PgMailboxStore {
    /// The conversations `q` asks for, newest first.
    pub async fn thread_rows(
        &self,
        q: &ThreadQuery<'_>,
    ) -> Result<Vec<ConversationSummary>, sqlx::Error> {
        let mut next = 2usize;
        let ids_param = q.thread_ids.map(|_| {
            next += 1;
            next - 1
        });
        let (scope_sql, category) = scope_predicate(&q.scope, next);
        if category.is_some() {
            next += 1;
        }
        let mut wheres = vec![scope_sql];
        if let Some(archived) = q.archived {
            wheres.push(format!("t.archived = {archived}"));
        }
        match q.held {
            Held::Hide => wheres.push("NOT t.quarantined".into()),
            Held::Only => wheres.push("t.quarantined".into()),
            Held::Include => {}
        }
        if q.sent {
            wheres.push("t.sent_count > 0".into());
        }
        if q.unread {
            wheres.push("t.unread_count > 0".into());
        }
        if q.starred {
            wheres.push("t.flagged".into());
        }
        let before_param = q.before_ts.map(|_| {
            next += 1;
            next - 1
        });
        if let Some(p) = before_param {
            wheres.push(format!("t.last_date < ${p}"));
        }
        let limit = q
            .limit
            .map(|_| format!(" LIMIT ${next}"))
            .unwrap_or_default();
        let sql = format!(
            "SELECT * FROM ({}) t WHERE {} ORDER BY t.last_date DESC, t.thread_id{limit}",
            thread_rows_sql(ids_param),
            wheres.join(" AND ")
        );
        let mut query = sqlx::query_as::<_, Row>(&sql).bind(q.user);
        if let Some(ids) = q.thread_ids {
            query = query.bind(ids.to_vec());
        }
        if let Some(c) = category {
            query = query.bind(c);
        }
        if let Some(ts) = q.before_ts {
            query = query.bind(ts);
        }
        if let Some(n) = q.limit {
            query = query.bind(n);
        }
        Ok(query
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
}
