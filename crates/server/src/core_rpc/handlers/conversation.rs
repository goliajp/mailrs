//! Handlers for `mailrs_core_api::method::conversation`.
//!
//! Every view here reads the store's thread rows through one translation of
//! the request's filter, [`thread_query`], the counterpart of the kevy core's
//! `threads_filter`, so a list, its unread badge, its category counts and
//! "mark all of this as read" cannot each select a different set.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use mailrs_core_api::method::conversation as wire;
use mailrs_core_api::types::{ConversationFilter, ConversationSummaryWire};
use mailrs_mailbox::pg::{Held, ThreadQuery};
use mailrs_mailbox::scope::{Scope, is_sent_folder, scope};

use crate::core_rpc::CoreRpcState;
use crate::core_rpc::handlers::failed;

/// The rows a list request asks for.
///
/// Archived is a tab, not a predicate inside the folder the client opened
/// it from, so an archived list drops the folder. Every conversation in
/// this store belongs to the deployment's own mail, whose account id is
/// the empty string: an account filter that does not name it selects
/// nothing.
pub(crate) fn thread_query<'a>(user: &'a str, f: &'a ConversationFilter) -> ThreadQuery<'a> {
    // the Trash too: a list of its own across folders
    let folder = if f.archived || f.trashed {
        None
    } else {
        f.folder.as_deref()
    };
    // the Trash shows what was moved there whether or not it had been
    // archived first; every other list leaves the Trash out
    let archived = match f.trashed {
        true => None,
        false => Some(f.archived),
    };
    let own_mail_excluded = f
        .accounts
        .as_ref()
        .is_some_and(|ids| !ids.iter().any(String::is_empty));
    let scope = if own_mail_excluded {
        Scope::Contradiction
    } else {
        scope(folder, f.category.as_deref())
    };
    ThreadQuery {
        scope,
        sent: is_sent_folder(folder),
        archived,
        trashed: Some(f.trashed),
        unread: f.unread.unwrap_or(false),
        starred: f.starred.unwrap_or(false),
        held: if f.include_quarantined {
            Held::Include
        } else {
            Held::Hide
        },
        before_ts: f.before_ts,
        limit: Some(i64::from(f.limit)),
        ..ThreadQuery::all(user)
    }
}

/// POST /v1/users/{user}/conversations:list
pub async fn list_conversations(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<wire::ListConversationsRequest>,
) -> Result<Json<wire::ListConversationsResponse>, StatusCode> {
    let rows = state
        .mailbox
        .thread_rows(&thread_query(&user, &req.filter))
        .await
        .map_err(failed("list_conversations", &user))?;
    let items: Vec<ConversationSummaryWire> = rows.iter().map(Into::into).collect();
    Ok(Json(wire::ListConversationsResponse { items }))
}

/// POST /v1/users/{user}/conversations:by-thread-ids — the reader's own
/// rows for those threads, archived and held included, in the order asked.
pub async fn conversations_by_thread_ids(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<wire::ConversationsByIdsRequest>,
) -> Result<Json<wire::ConversationsByIdsResponse>, StatusCode> {
    let ids: Vec<String> = req.thread_ids.iter().map(|t| t.to_string()).collect();
    let q = ThreadQuery {
        archived: None,
        held: Held::Include,
        thread_ids: Some(&ids),
        ..ThreadQuery::all(&user)
    };
    let mut rows = state
        .mailbox
        .thread_rows(&q)
        .await
        .map_err(failed("conversations_by_thread_ids", &user))?;
    let position = |tid: &str| ids.iter().position(|i| i == tid).unwrap_or(usize::MAX);
    rows.sort_by_key(|r| position(&r.thread_id));
    let items: Vec<ConversationSummaryWire> = rows.iter().map(Into::into).collect();
    Ok(Json(wire::ConversationsByIdsResponse { items }))
}

/// GET /v1/users/{user}/conversations/categories — live conversations per
/// triage class. Junk mail is not a class: its one surface is the Junk
/// folder.
pub async fn conversation_categories(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
) -> Result<Json<wire::ConversationCategoriesResponse>, StatusCode> {
    let q = ThreadQuery {
        held: Held::Include,
        ..ThreadQuery::all(&user)
    };
    let rows = state
        .mailbox
        .thread_rows(&q)
        .await
        .map_err(failed("conversation_categories", &user))?;
    let categories = mailrs_triage::CLASSES
        .into_iter()
        .map(|class| wire::CategoryCount {
            category: class.to_string(),
            count: rows
                .iter()
                .filter(|r| r.category.eq_ignore_ascii_case(class))
                .count() as i64,
        })
        .filter(|c| c.count > 0)
        .collect();
    Ok(Json(wire::ConversationCategoriesResponse { categories }))
}

/// GET /v1/users/{user}/conversations/unseen-count — live conversations
/// outside Junk with unread mail.
pub async fn unseen_count(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
) -> Result<Json<wire::UnseenCountResponse>, StatusCode> {
    let q = ThreadQuery {
        scope: Scope::NotJunk,
        unread: true,
        ..ThreadQuery::all(&user)
    };
    let rows = state
        .mailbox
        .thread_rows(&q)
        .await
        .map_err(failed("unseen_count", &user))?;
    Ok(Json(wire::UnseenCountResponse {
        count: rows.len() as i64,
    }))
}
