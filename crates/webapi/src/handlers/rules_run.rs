//! A rule over mail already received: which conversations it would change,
//! changing them, and undoing that.
//!
//! Matching is the search the rule's query is, so the count a preview
//! shows is the count a run changes. Junk and the Trash are left out:
//! a rule cannot vouch for mail in Junk, and the Trash is on its way out.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::error::CoreApiError;
use mailrs_core_api::method::conversation::SearchConversationsRequest;
use mailrs_core_api::types::ConversationSummaryWire;
use mailrs_core_sidestate::families::rules::{self as stored, Before, Run};
use mailrs_mailbox::rules::{Action, check};
use mailrs_mailbox::scope::{Bucket, bucket_of};

use super::rules::{Fail, refused};
use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

/// Most conversations one run changes.
const RUN_MAX: u32 = 10_000;
/// Rows a preview shows.
const PREVIEW_ROWS: usize = 20;

/// The conversations `query` finds, archived ones included, out of Junk.
async fn matching(
    state: &WebState,
    user: &str,
    query: &str,
) -> Result<Vec<ConversationSummaryWire>, CoreApiError> {
    let mut out: Vec<ConversationSummaryWire> = Vec::new();
    for archived in [false, true] {
        let req = SearchConversationsRequest {
            query: query.to_string(),
            category: None,
            limit: RUN_MAX,
            folder: None,
            unread: None,
            starred: None,
            archived,
        };
        let found = state.core.search_conversations(user, &req).await?;
        out.extend(found.items);
    }
    out.retain(|r| bucket_of(&r.category) != Bucket::Junk && r.trashed_at == 0);
    out.sort_by_key(|r| std::cmp::Reverse(r.last_date));
    out.dedup_by(|a, b| a.thread_id == b.thread_id);
    Ok(out)
}

#[derive(Debug, serde::Deserialize)]
pub struct PreviewRequest {
    pub query: String,
}

/// How many conversations a rule with `query` would change if run now,
/// and the newest of them.
pub(crate) async fn preview_rows(
    state: &WebState,
    user: &str,
    query: &str,
) -> Result<(usize, Vec<ConversationSummaryWire>), Fail> {
    // any action: only the query is being checked
    check(query, &["read".to_string()]).map_err(|why| (StatusCode::BAD_REQUEST, why.code()))?;
    let rows = matching(state, user, query)
        .await
        .map_err(|_| (StatusCode::BAD_GATEWAY, "search_failed"))?;
    let count = rows.len();
    Ok((count, rows.into_iter().take(PREVIEW_ROWS).collect()))
}

/// POST /api/rules/preview `{query}`.
pub async fn preview(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<PreviewRequest>,
) -> Response {
    match preview_rows(&state, &user, &req.query).await {
        Ok((count, rows)) => {
            let items: Vec<crate::handlers::conversations::ConversationResponse> =
                rows.into_iter().map(Into::into).collect();
            Json(serde_json::json!({ "count": count, "items": items })).into_response()
        }
        Err((status, code)) => refused(code, status),
    }
}

fn before(row: &ConversationSummaryWire, actions: &[Action]) -> Before {
    let labels = actions
        .iter()
        .filter_map(|a| match a {
            Action::Label(id) if row.labels.contains(id) => Some(id.clone()),
            _ => None,
        })
        .collect();
    Before {
        thread_id: row.thread_id.clone(),
        archived: row.archived,
        unread: row.unread_count > 0,
        starred: row.flagged,
        junk: false,
        trashed: false,
        labels,
    }
}

async fn act(state: &WebState, user: &str, tid: &str, action: &Action) -> Result<(), CoreApiError> {
    let f = &state.core;
    match action {
        Action::Label(id) => f.set_thread_label(user, tid, id, true).await,
        Action::Archive => f.archive_thread(user, tid).await.map(|_| ()),
        Action::Read => f.mark_thread_read(user, tid).await.map(|_| ()),
        Action::Star => f.star_thread(user, tid).await.map(|_| ()),
        Action::Junk => f.mark_junk(user, tid).await.map(|_| ()),
        Action::Trash => f.trash_thread(user, tid).await.map(|_| ()),
    }
}

/// Put back what `action` changed, from how the conversation stood.
async fn revert(
    state: &WebState,
    user: &str,
    was: &Before,
    action: &Action,
) -> Result<(), CoreApiError> {
    let (f, tid) = (&state.core, was.thread_id.as_str());
    match action {
        Action::Label(id) if !was.labels.contains(id) => {
            f.set_thread_label(user, tid, id, false).await
        }
        Action::Archive if !was.archived => f.unarchive_thread(user, tid).await.map(|_| ()),
        Action::Read if was.unread => f.mark_thread_unread(user, tid).await.map(|_| ()),
        Action::Star if !was.starred => f.unstar_thread(user, tid).await.map(|_| ()),
        Action::Junk if !was.junk => f.move_to_inbox(user, tid).await.map(|_| ()),
        Action::Trash if !was.trashed => f.untrash_thread(user, tid).await.map(|_| ()),
        _ => Ok(()),
    }
}

/// Run rule `id` over mail already received. Answers how many
/// conversations it changed and the run's id, which undoes it for seven days.
pub(crate) async fn run(state: &WebState, user: &str, id: &str) -> Result<serde_json::Value, Fail> {
    let who = user.to_string();
    let rules = with_kevy(move |c| stored::list(c, &who)).map_err(|s| (s, "store_failed"))?;
    let Some(rule) = rules.into_iter().find(|r| r.id == id) else {
        return Err((StatusCode::NOT_FOUND, "not_found"));
    };
    let actions: Vec<Action> = rule
        .actions
        .iter()
        .filter_map(|s| Action::parse(s))
        .collect();
    let rows = matching(state, user, &rule.query)
        .await
        .map_err(|_| (StatusCode::BAD_GATEWAY, "search_failed"))?;
    let mut changed: Vec<Before> = Vec::new();
    let mut failed = 0u32;
    for row in &rows {
        let was = before(row, &actions);
        let mut ok = true;
        for action in &actions {
            if let Err(e) = act(state, user, &row.thread_id, action).await {
                tracing::warn!(error = %e, %user, thread_id = %row.thread_id, action = %action.spelling(), "rule run: action failed");
                ok = false;
            }
        }
        failed += u32::from(!ok);
        changed.push(was);
    }
    let run = Run {
        rule_id: rule.id,
        actions: rule.actions,
        before: changed,
    };
    let count = run.before.len();
    let who = user.to_string();
    let run_id =
        with_kevy(move |c| stored::record_run(c, &who, &run)).map_err(|s| (s, "store_failed"))?;
    Ok(serde_json::json!({ "run_id": run_id, "changed": count, "failed": failed }))
}

/// POST /api/rules/{id}/apply
pub async fn apply(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<String>,
) -> Response {
    match run(&state, &user, &id).await {
        Ok(v) => Json(v).into_response(),
        Err((status, code)) => refused(code, status),
    }
}

/// Put every conversation a run changed back as it was. Once: the run is
/// gone after.
pub(crate) async fn undo_run(
    state: &WebState,
    user: &str,
    run_id: &str,
) -> Result<serde_json::Value, Fail> {
    let (who, run_id) = (user.to_string(), run_id.to_string());
    let run = with_kevy(move |c| stored::take_run(c, &who, &run_id))
        .map_err(|s| (s, "store_failed"))?
        .ok_or((StatusCode::NOT_FOUND, "not_found"))?;
    let actions: Vec<Action> = run
        .actions
        .iter()
        .filter_map(|s| Action::parse(s))
        .collect();
    let mut failed = 0u32;
    for was in &run.before {
        for action in &actions {
            if let Err(e) = revert(state, user, was, action).await {
                tracing::warn!(error = %e, %user, thread_id = %was.thread_id, "rule undo: revert failed");
                failed += 1;
            }
        }
    }
    Ok(serde_json::json!({ "restored": run.before.len(), "failed": failed }))
}

/// POST /api/rules/runs/{run_id}/undo
pub async fn undo(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(run_id): Path<String>,
) -> Response {
    match undo_run(&state, &user, &run_id).await {
        Ok(v) => Json(v).into_response(),
        Err((status, code)) => refused(code, status),
    }
}
