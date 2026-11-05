//! `POST /v1/reports`.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};

use crate::app::App;
use crate::readings::Rejection;

pub(super) async fn receive(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let signature = headers
        .get(mailrs_ops_report::wire::SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    match app.book.receive(&body, signature, app.clock.now()) {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(Rejection::Unreadable) => StatusCode::BAD_REQUEST,
        Err(Rejection::UnknownProcess | Rejection::BadSignature) => StatusCode::UNAUTHORIZED,
        Err(Rejection::Replayed) => StatusCode::CONFLICT,
        Err(Rejection::Clock) => StatusCode::UNPROCESSABLE_ENTITY,
        Err(Rejection::NoMasterKey | Rejection::Store) => StatusCode::SERVICE_UNAVAILABLE,
    }
}
