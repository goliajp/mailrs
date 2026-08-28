//! The review screen: what was held as suspected fraud, and letting
//! one go.
//!
//! Held mail is kept. It is the evidence the abuse reports are built
//! from — the ones sent on 2026-08-27 attached the originals — and it
//! is out of the ordinary lists so that an attempt to defraud somebody
//! is not something they run into by accident.

use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::{Json, response::IntoResponse};

use super::conversations::{AuthedUser, ConversationResponse, map_err};
use crate::WebState;

/// What the review list may be narrowed by. Paged like every other
/// list — the client appends pages, so a route that ignored the cursor
/// would show every held conversation twice.
#[derive(Debug, serde::Deserialize)]
pub struct QuarantineQuery {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default, alias = "before")]
    pub before_ts: Option<i64>,
}

/// GET /api/quarantine — the held conversations, newest first.
pub async fn list_quarantine(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    axum::extract::Query(q): axum::extract::Query<QuarantineQuery>,
) -> Result<Json<Vec<ConversationResponse>>, StatusCode> {
    let resp = state
        .core
        .list_quarantined(&user, q.limit.unwrap_or(50).clamp(1, 200), q.before_ts)
        .await
        .map_err(map_err)?;
    Ok(Json(resp.items.into_iter().map(Into::into).collect()))
}

/// POST /api/quarantine/{thread_id}/release — it was not fraud.
pub async fn release_quarantine(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .release_quarantined(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// GET /api/messages/{message_id}/fraud-verdict — the four layers, as
/// they were decided.
///
/// `{"verdict": null}` for a message nobody suspected. Null rather
/// than an empty verdict: "nothing was found" and "it was examined and
/// cleared" are different facts, and a screen must not draw them the
/// same way.
pub async fn get_fraud_verdict(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(message_id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let v = state
        .core
        .fraud_verdict(&user, &message_id)
        .await
        .map_err(map_err)?;
    Ok(Json(v))
}
