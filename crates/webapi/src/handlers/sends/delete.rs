//! Deleting a send the sender is finished with.

use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;

use crate::WebState;
use crate::handlers::conversations::{AuthedUser, map_err};
use crate::handlers::kevy_util::with_kevy;

use mailrs_core_api::method::admin::SetMessageFlagsRequest;
use mailrs_core_api::method::message::FLAG_DELETED;
use mailrs_core_sidestate::families::send as sendfam;
use mailrs_core_sidestate::families::send_read;

/// `DELETE /api/mail/sends/{send_id}`
///
/// Removes the send, every resend of it, and the sender's own copy of the
/// message — the Send list shows a row while either exists, so taking
/// away one of them leaves the row in place.
///
/// 409 while the send is still going out or scheduled: deleting the row
/// would hide mail that is about to be delivered. Cancel it first.
pub async fn delete_send(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(send_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let u = user.clone();
    let s = send_id.clone();
    let item = with_kevy(move |c| send_read::read_one(c, &u, &s))?.ok_or(StatusCode::NOT_FOUND)?;
    if matches!(
        item.status,
        sendfam::Status::Sending | sendfam::Status::Scheduled
    ) {
        return Err(StatusCode::CONFLICT);
    }

    let message_id = send_id
        .split_once("#r")
        .map_or(send_id.as_str(), |(m, _)| m);
    match state
        .core
        .find_by_message_id_for_user(&user, message_id)
        .await
    {
        Ok(copy) => {
            let req = SetMessageFlagsRequest {
                flags: copy.flags | FLAG_DELETED,
            };
            state
                .core
                .set_message_flags(&user, copy.uid, &req)
                .await
                .map_err(map_err)?;
        }
        // the copy is already gone, e.g. its conversation was deleted
        Err(e) if e.status_code() == 404 => {}
        Err(e) => return Err(map_err(e)),
    }

    let u = user.clone();
    let s = send_id.clone();
    let removed = with_kevy(move |c| sendfam::delete_send_chain(c, &u, &s))?;
    tracing::info!(%user, %send_id, removed, "send deleted");
    Ok(StatusCode::NO_CONTENT)
}
