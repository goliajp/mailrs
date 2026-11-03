//! Stopping a send that has not gone out.
//!
//! Two writes, in this order: take the jobs out of the queue, then
//! mark the recipients. If the second half fails the message still
//! does not go — a row that says `sending` about a queue with nothing
//! in it is a wrong label, and a queue still holding a job the sender
//! asked to stop is wrong mail. Given a choice between the two, the
//! one that does not send the mail is the one to be wrong in.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

use mailrs_core_sidestate::families::outbound;
use mailrs_core_sidestate::families::send as sendfam;
use mailrs_core_sidestate::families::send_read;

/// What the caller is told, in the three separate numbers it is made
/// of. A single "cancelled: true" would hide the case that matters —
/// some recipients already had it.
#[derive(serde::Serialize)]
pub struct CancelResponse {
    /// Queue jobs removed before anything was attempted.
    pub jobs_removed: usize,
    /// Recipients moved to a terminal cancelled state.
    pub recipients_cancelled: usize,
    /// Recipients an MX had already accepted. **Not recalled** — the
    /// message is with them and nothing here can take it back.
    pub already_delivered: usize,
}

/// `POST /api/mail/sends/{send_id}/cancel`
///
/// 404 when there is no such send, 409 when it has already finished —
/// a cancel that answered 200 about a delivered message would be the
/// one lie this endpoint must not tell.
pub async fn cancel_send(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(send_id): Path<String>,
) -> Result<Json<CancelResponse>, StatusCode> {
    cancel(&user, &send_id).map(Json)
}

/// Stop `user`'s send `send_id`: 404 when there is none, 409 when it has
/// finished. Shared by the web route and JMAP's undo.
pub(crate) fn cancel(user: &str, send_id: &str) -> Result<CancelResponse, StatusCode> {
    let (u, s) = (user.to_string(), send_id.to_string());
    let item = with_kevy(move |c| send_read::read_one(c, &u, &s))?.ok_or(StatusCode::NOT_FOUND)?;
    match item.status {
        sendfam::Status::Sending | sendfam::Status::Scheduled => {}
        // Delivered, failed, partial, already cancelled: there is
        // nothing in flight to stop, and saying "cancelled" would
        // claim something untrue about mail that has landed.
        _ => return Err(StatusCode::CONFLICT),
    }

    let s = send_id.to_string();
    let sender = user.to_string();
    let jobs_removed = with_kevy(move |c| Ok(outbound::drop_queued_jobs(c, &s, &sender)))?.len();

    let (u, s) = (user.to_string(), send_id.to_string());
    let (recipients_cancelled, already_delivered) =
        with_kevy(move |c| sendfam::cancel_send(c, &u, &s, outbound::CANCELLED_REASON))?;

    tracing::info!(
        %user, %send_id, jobs_removed, recipients_cancelled, already_delivered,
        "send cancelled"
    );
    Ok(CancelResponse {
        jobs_removed,
        recipients_cancelled,
        already_delivered,
    })
}
