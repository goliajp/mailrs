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
use mailrs_mailbox::scope::{Bucket, bucket_of};
use mailrs_mailbox::types::ConversationSummary;
use std::collections::HashMap;

use crate::core_rpc::CoreRpcState;
use crate::core_rpc::handlers::conversation::{labelled_query, thread_query};
use crate::core_rpc::handlers::failed;

/// POST /v1/users/{user}/conversations:search — the matches in the list
/// the search was made from, selected by the list's own [`thread_query`],
/// or in the place its `in:` names; best match first when there are
/// words, newest first when there are only operators.
pub async fn search_conversations(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<conv::SearchConversationsRequest>,
) -> Result<Json<conv::SearchConversationsResponse>, StatusCode> {
    let limit = req.limit();
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
                .search_thread_ids(&user, &query.text, req.text_hits_wanted() as i64)
                .await
                .map_err(failed("search", &user))?,
        ),
    };
    // a label is a set of ids, so its list is the matches among them
    let label = match (query.within, req.quarantined) {
        (None, false) => req.filter.label.as_deref(),
        _ => None,
    };
    let narrowed = match label {
        Some(id) => Some(labelled_matches(&state, &user, id, ids.as_deref()).await?),
        None => ids.clone(),
    };
    let q = scoped_query(&user, &req, query.within.is_some(), narrowed.as_deref());
    let mut rows = state
        .mailbox
        .thread_rows(&q)
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

/// The conversations carrying label `id`, among the text matches when
/// there are words.
async fn labelled_matches(
    state: &CoreRpcState,
    user: &str,
    id: &str,
    matches: Option<&[String]>,
) -> Result<Vec<String>, StatusCode> {
    let labelled = state
        .mailbox
        .threads_with_label(user, id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, %user, "search: threads_with_label failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(match matches {
        Some(ids) => ids
            .iter()
            .filter(|i| labelled.contains(i))
            .cloned()
            .collect(),
        None => labelled,
    })
}

/// The rows a search looks among: the list it was made from, read as the
/// list reads it but in one page; Review; or, with `in:`, everywhere, to
/// be narrowed to the place it names. `ids` are the candidates — the text
/// matches, or a label's conversations — when there are any.
fn scoped_query<'a>(
    user: &'a str,
    req: &'a conv::SearchConversationsRequest,
    within: bool,
    ids: Option<&'a [String]>,
) -> ThreadQuery<'a> {
    if within {
        return ThreadQuery {
            archived: None,
            trashed: None,
            thread_ids: ids,
            ..ThreadQuery::all(user)
        };
    }
    // the Review list, as `list_quarantined` reads it
    if req.quarantined {
        return ThreadQuery {
            held: Held::Only,
            thread_ids: ids,
            ..ThreadQuery::all(user)
        };
    }
    let listed = thread_query(user, &req.filter);
    let listed = match (ids, req.filter.label.is_some()) {
        (Some(ids), true) => labelled_query(listed, ids),
        (ids, _) => ThreadQuery {
            thread_ids: ids,
            ..listed
        },
    };
    ThreadQuery {
        before_ts: None,
        limit: None,
        ..listed
    }
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
