//! `POST /api/senders/accept`: the reader accepts first contacts, which
//! leave the New section of the sectioned inbox.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use mailrs_core_api::method::senders::{AcceptSendersRequest, AcceptSendersResponse};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

/// The request body: bare addresses.
#[derive(Debug, serde::Deserialize)]
pub struct AcceptBody {
    pub addresses: Vec<String>,
}

/// POST /api/senders/accept
pub async fn accept_senders(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(body): Json<AcceptBody>,
) -> Result<Json<AcceptSendersResponse>, StatusCode> {
    if body.addresses.is_empty() || body.addresses.len() > 500 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let req = AcceptSendersRequest {
        addresses: body.addresses,
        accepted_at: chrono::Utc::now().timestamp(),
    };
    state
        .core
        .accept_senders(&user, &req)
        .await
        .map(Json)
        .map_err(|e| {
            StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
        })
}
