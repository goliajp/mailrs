//! Fetching a thread's remote images on request, as fastcore does: never
//! for a thread in Junk or held for review.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mailrs_core_api::method::thread as wire;
use mailrs_mailbox::pg::{Held, ThreadQuery};
use mailrs_mailbox::scope::{Bucket, bucket_of};

use crate::core_rpc::CoreRpcState;

fn fail(e: impl std::fmt::Display) -> StatusCode {
    tracing::warn!(error = %e, "images not queued");
    StatusCode::INTERNAL_SERVER_ERROR
}

/// POST /v1/users/{user}/threads/{thread_id}/images
pub async fn prefetch_images(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let ids = [thread_id.clone()];
    let q = ThreadQuery {
        archived: None,
        trashed: None,
        held: Held::Include,
        thread_ids: Some(&ids),
        ..ThreadQuery::all(&user)
    };
    let rows = state.mailbox.thread_rows(&q).await.map_err(fail)?;
    let Some(row) = rows.first() else {
        return Err(StatusCode::NOT_FOUND);
    };
    if row.quarantined || bucket_of(&row.category) == Bucket::Junk {
        return Err(StatusCode::CONFLICT);
    }
    if let Some(url) = state.net_url.clone() {
        let raws = state
            .mailbox
            .thread_raw_messages(&user, &thread_id, &state.maildir_root)
            .await
            .map_err(fail)?;
        let q = mailrs_intake::image_prefetch::NetworkQueue { url };
        mailrs_intake::image_prefetch::queue_thread(&q, &user, &thread_id, &raws);
    }
    Ok(Json(wire::ThreadActionResponse {
        affected: 1,
        new_modseq: 0,
    }))
}
