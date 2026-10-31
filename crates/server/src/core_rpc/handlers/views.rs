//! Search, the sent list, finding a thread by Message-ID, and marking a
//! whole view as read.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use mailrs_core_api::method::conversation as conv;
use mailrs_core_api::method::thread as wire;
use mailrs_core_api::types::ConversationSummaryWire;
use mailrs_mailbox::pg::{Held, ThreadQuery};
use mailrs_mailbox::query::{Cond, Place, Query, ThreadFacts};
use mailrs_mailbox::scope::{Bucket, bucket_of, is_sent_folder, scope};
use mailrs_mailbox::types::ConversationSummary;
use std::collections::HashMap;

use crate::core_rpc::CoreRpcState;
use crate::core_rpc::handlers::conversation::thread_query;
use crate::core_rpc::handlers::failed;

/// How many more matches than the page to ask the text search for, so the
/// page is still full after the view's scope has dropped some.
const OVERFETCH: i64 = 8;

/// POST /v1/users/{user}/conversations:search — the matches in the view
/// the search was made from, or in the place its `in:` names; best match
/// first when there are words, newest first when there are only
/// operators. Held mail is out of search as it is out of every list.
pub async fn search_conversations(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<conv::SearchConversationsRequest>,
) -> Result<Json<conv::SearchConversationsResponse>, StatusCode> {
    let limit = match req.limit {
        0 => 20,
        n => n as usize,
    };
    let query = Query::parse(&req.query);
    if query.text.is_empty() && !query.has_operators() {
        return Ok(Json(conv::SearchConversationsResponse {
            items: Vec::new(),
        }));
    }
    let ids = match query.text.is_empty() {
        true => None,
        false => Some(
            state
                .mailbox
                .search_thread_ids(&user, &query.text, limit as i64 * OVERFETCH)
                .await
                .map_err(failed("search", &user))?,
        ),
    };
    let folder = req.folder.as_deref();
    let q = match query.within {
        // the view the search was made from
        None => ThreadQuery {
            scope: scope(folder, req.category.as_deref()),
            sent: is_sent_folder(folder),
            archived: Some(req.archived),
            unread: req.unread.unwrap_or(false),
            starred: req.starred.unwrap_or(false),
            thread_ids: ids.as_deref(),
            ..ThreadQuery::all(&user)
        },
        // everywhere, narrowed below to the place `in:` names
        Some(_) => ThreadQuery {
            archived: None,
            trashed: None,
            thread_ids: ids.as_deref(),
            ..ThreadQuery::all(&user)
        },
    };
    let mut rows = state
        .mailbox
        .thread_rows(&ThreadQuery {
            held: Held::Hide,
            ..q
        })
        .await
        .map_err(failed("search", &user))?;
    match &ids {
        Some(ids) => {
            let rank = |tid: &str| ids.iter().position(|i| i == tid).unwrap_or(usize::MAX);
            rows.sort_by_key(|r| rank(&r.thread_id));
        }
        None => rows.sort_by(|a, b| {
            b.last_date
                .cmp(&a.last_date)
                .then_with(|| a.thread_id.cmp(&b.thread_id))
        }),
    }
    if let Some(within) = query.within {
        rows.retain(|r| within.admits(place_of(r)));
    }
    let names = label_names(&state, &user, &query).await?;
    let mut kept = Vec::new();
    for row in rows {
        if kept.len() >= limit {
            break;
        }
        let messages = match query.needs_messages() {
            true => state
                .mailbox
                .message_facts(&user, &row.thread_id)
                .await
                .map_err(failed("search", &user))?,
            false => Vec::new(),
        };
        let labels = names.get(&row.thread_id).cloned().unwrap_or_default();
        let facts = ThreadFacts {
            subject: &row.subject,
            senders: &row.participants,
            latest_date: row.last_date,
            unread_count: row.unread_count as i64,
            starred: row.flagged,
            pinned: row.pinned,
            labels: &labels,
        };
        if query.admits(&facts, || messages) {
            kept.push(row);
        }
    }
    let items: Vec<ConversationSummaryWire> = kept.iter().map(Into::into).collect();
    let items = super::labels::with_labels(&state, &user, items).await;
    Ok(Json(conv::SearchConversationsResponse { items }))
}

fn place_of(r: &ConversationSummary) -> Place {
    Place {
        archived: r.archived,
        trashed: r.trashed_at > 0,
        junk: bucket_of(&r.category) == Bucket::Junk,
        has_sent: r.sent_count > 0,
        sent_only: r.message_count > 0 && r.sent_count >= r.message_count,
        starred: r.flagged,
    }
}

/// The names of the labels on each conversation, read only when the
/// query asks about labels.
async fn label_names(
    state: &CoreRpcState,
    user: &str,
    query: &Query,
) -> Result<HashMap<String, Vec<String>>, StatusCode> {
    let asks = query.conds.iter().any(|(_, c)| matches!(c, Cond::Label(_)));
    if !asks {
        return Ok(HashMap::new());
    }
    let defs: HashMap<String, String> = state
        .mailbox
        .labels(user)
        .await
        .map_err(failed("search", user))?
        .into_iter()
        .map(|l| (l.id, l.name))
        .collect();
    Ok(state
        .mailbox
        .labels_by_thread(user)
        .await
        .map_err(failed("search", user))?
        .into_iter()
        .map(|(tid, ids)| {
            let names = ids.iter().filter_map(|id| defs.get(id).cloned()).collect();
            (tid, names)
        })
        .collect())
}

/// GET /v1/users/{user}/sent-messages — what the user sent, newest first.
pub async fn list_sent_messages(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
) -> Result<Json<wire::SentMessagesResponse>, StatusCode> {
    let rows = state
        .mailbox
        .sent_messages(&user)
        .await
        .map_err(failed("sent_messages", &user))?;
    let items = rows
        .into_iter()
        .map(
            |(uid, message_id, thread_id, to, subject, internal_date)| wire::SentMessageSummary {
                uid: uid as u32,
                message_id,
                thread_id,
                to,
                subject,
                internal_date,
            },
        )
        .collect();
    Ok(Json(wire::SentMessagesResponse { items }))
}

/// POST /v1/users/{user}/conversations:mark-all-read
pub async fn mark_all_read(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let flipped = state
        .mailbox
        .mark_all_read(&user)
        .await
        .map_err(failed("mark_all_read", &user))?;
    Ok(Json(serde_json::json!({ "ok": true, "flipped": flipped })))
}

/// POST /v1/users/{user}/conversations:mark-list-read — marks the rows the
/// list shows, through the list's own query. Every row, not one page.
pub async fn mark_list_read(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<conv::ListConversationsRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let q = ThreadQuery {
        limit: None,
        ..thread_query(&user, &req.filter)
    };
    let flipped = state
        .mailbox
        .mark_rows_read(&q)
        .await
        .map_err(failed("mark_list_read", &user))?;
    Ok(Json(serde_json::json!({ "ok": true, "flipped": flipped })))
}
