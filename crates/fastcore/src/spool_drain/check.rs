//! `GET /v1/admin/recipients/{address}/deliverable`: the receiver asks at
//! RCPT, so mail the drain could never deliver is refused while the sender
//! is still connected instead of bounced later to a sender that is usually
//! forged. Answers from the same `route` the drain delivers along, and
//! writes nothing — not even the maildir a fresh account gets on delivery.

use std::io;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::admin::RecipientDeliverableResponse;
use mailrs_core_sidestate::families::mock;

use mailrs_intake::route::{Route, route};

use super::core::Fastcore;
use super::deliver::canonical_recipient;
use crate::FastcoreState;

pub(crate) async fn recipient_deliverable_route(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
) -> Response {
    let maildir_root =
        std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".to_string());
    let answer =
        tokio::task::spawn_blocking(move || deliverable(&state, &maildir_root, &address)).await;
    match answer {
        Ok(Ok(deliverable)) => Json(RecipientDeliverableResponse { deliverable }).into_response(),
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "recipient check: a store could not be read");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::warn!(error = %e, "recipient check task failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Whether the drain would take mail for `raw`. Mock addresses always
/// are: the mock store takes them, whatever is behind them.
fn deliverable(state: &Arc<FastcoreState>, maildir_root: &str, raw: &str) -> io::Result<bool> {
    let fwd = canonical_recipient(raw);
    if mock::is_mock(&fwd) {
        return Ok(true);
    }
    Ok(!matches!(
        route(&Fastcore::new(state, maildir_root), raw, &fwd)?,
        Route::Nobody
    ))
}
