//! Filing a conversation into a triage bucket, and what the reader's choice
//! teaches the classifiers.
//!
//! The same side effects as the kevy core: "junk" credits the correspondent
//! with a junk mark and learns the thread as spam, "not junk" learns it as
//! ham, and filing into Inbox, Notifications or Promotions teaches the
//! triage corpus that class. The corpora live in the network kevy both
//! cores share. Training follows the reader's action and never fails it.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use mailrs_core_api::method::thread as wire;
use mailrs_core_sidestate::NetKevy;
use mailrs_core_sidestate::families::contacts::Engagement;
use mailrs_mailbox::pg::{Held, ThreadQuery};
use mailrs_mailbox::scope::Bucket;

use crate::core_rpc::CoreRpcState;

/// What filing teaches besides moving the thread.
enum Lesson {
    Spam,
    Ham,
    Class(&'static str),
}

async fn file(
    state: &CoreRpcState,
    user: &str,
    thread_id: &str,
    bucket: Bucket,
    lesson: Lesson,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let fail = |e: sqlx::Error| {
        tracing::warn!(error = %e, %user, %thread_id, "filing failed");
        StatusCode::INTERNAL_SERVER_ERROR
    };
    if matches!(lesson, Lesson::Spam) {
        credit_junk_mark(state, user, thread_id)
            .await
            .map_err(fail)?;
    }
    let affected = state
        .mailbox
        .file_thread(user, thread_id, bucket)
        .await
        .map_err(fail)?;
    // nothing of the caller's in the thread: not theirs to file
    if affected == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    let raws = state
        .mailbox
        .thread_raw_messages(user, thread_id, &state.maildir_root)
        .await
        .map_err(fail)?;
    // a thread filed into the Inbox may come from Junk, where its images
    // were not fetched
    if matches!(bucket, Bucket::Inbox)
        && let Some(url) = state.net_url.clone()
    {
        let q = mailrs_intake::image_prefetch::NetworkQueue { url };
        mailrs_intake::image_prefetch::queue_thread(&q, user, thread_id, &raws);
    }
    if let Some(mut conn) = state.net_conn() {
        match lesson {
            Lesson::Spam => mailrs_triage::spam::train(&mut conn, thread_id, &raws, true),
            Lesson::Ham => mailrs_triage::spam::train(&mut conn, thread_id, &raws, false),
            Lesson::Class(c) => mailrs_triage::spam::train_class(&mut conn, thread_id, &raws, c),
        }
    }
    Ok(Json(wire::ThreadActionResponse {
        affected,
        new_modseq: 0,
    }))
}

/// Mark the conversation's correspondent as someone this reader files as
/// junk. Read before the filing, while the thread still has its senders.
async fn credit_junk_mark(
    state: &CoreRpcState,
    user: &str,
    thread_id: &str,
) -> Result<(), sqlx::Error> {
    let ids = [thread_id.to_string()];
    let q = ThreadQuery {
        archived: None,
        held: Held::Include,
        thread_ids: Some(&ids),
        ..ThreadQuery::all(user)
    };
    let Some(row) = state.mailbox.thread_rows(&q).await?.into_iter().next() else {
        return Ok(());
    };
    if let Some(mut conn) = state.net_conn()
        && let Err(e) = mailrs_core_sidestate::families::correspondent::record_thread_engagement(
            &mut conn,
            user,
            &row.participants,
            Engagement::MarkedJunk,
        )
    {
        tracing::warn!(error = %e, %user, %thread_id, "engagement not recorded");
    }
    Ok(())
}

/// POST /v1/users/{user}/threads/{thread_id}/mark-junk
pub async fn mark_junk(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    file(&state, &user, &thread_id, Bucket::Junk, Lesson::Spam).await
}

/// POST /v1/users/{user}/threads/{thread_id}/mark-not-junk
pub async fn mark_not_junk(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    file(&state, &user, &thread_id, Bucket::Inbox, Lesson::Ham).await
}

/// POST /v1/users/{user}/threads/{thread_id}/mark-notification
pub async fn mark_notification(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    file(
        &state,
        &user,
        &thread_id,
        Bucket::Notifications,
        Lesson::Class("notification"),
    )
    .await
}

/// POST /v1/users/{user}/threads/{thread_id}/mark-promotion
pub async fn mark_promotion(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    file(
        &state,
        &user,
        &thread_id,
        Bucket::Promotions,
        Lesson::Class("promotion"),
    )
    .await
}

/// POST /v1/users/{user}/threads/{thread_id}/move-to-inbox
pub async fn move_to_inbox(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    file(
        &state,
        &user,
        &thread_id,
        Bucket::Inbox,
        Lesson::Class("inbox"),
    )
    .await
}
