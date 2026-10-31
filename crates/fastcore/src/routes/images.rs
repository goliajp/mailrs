//! Fetching a thread's remote images on request, for mail that arrived
//! before images were fetched on arrival. Never for a thread in Junk or held
//! for review: fetching a sender's images tells them the address is read.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_mailbox_kevy::scope::{Bucket, bucket_of};

use crate::FastcoreState;
use crate::routes::message_ops::action_result;

/// `?anyway=true`: the reader asked for a thread in Junk or held all the same.
#[derive(serde::Deserialize, Default)]
pub(crate) struct Anyway {
    #[serde(default)]
    anyway: bool,
}

/// `POST /v1/users/{user}/threads/{thread_id}/images`.
pub(crate) async fn prefetch_images(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
    Query(q): Query<Anyway>,
) -> Response {
    let Ok(Some(row)) = state.mailbox.get_thread_for_user(&user, &thread_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !q.anyway && (row.quarantined || bucket_of(&row.category) == Bucket::Junk) {
        return StatusCode::CONFLICT.into_response();
    }
    crate::image_prefetch::queue_thread(&state, &user, &thread_id);
    action_result(true)
}
