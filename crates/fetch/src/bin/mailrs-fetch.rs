//! `mailrs-fetch` — fetches one remote image per request for fastcore.
//!
//! `POST /fetch {"url": …}` answers the re-encoded image, or 422 with why
//! it was refused. It holds no secret and reaches nothing of mailrs: it
//! sits on a network shared with fastcore alone, and its own checks refuse
//! every private address, that network's included.

use std::sync::Arc;

use axum::Json;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use tokio::sync::Semaphore;

/// Fetches in flight at once; the rest wait.
const CONCURRENT: usize = 8;

#[derive(serde::Deserialize)]
struct FetchRequest {
    url: String,
}

#[tokio::main]
async fn main() {
    let bind = std::env::var("MAILRS_FETCH_BIND").unwrap_or_else(|_| "0.0.0.0:3500".into());
    let slots = Arc::new(Semaphore::new(CONCURRENT));
    let app = axum::Router::new().route(
        "/fetch",
        post(move |Json(req): Json<FetchRequest>| {
            let slots = slots.clone();
            async move { fetch(&slots, &req.url).await }
        }),
    );
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .unwrap_or_else(|e| panic!("bind {bind}: {e}"));
    eprintln!("mailrs-fetch listening on {bind}");
    axum::serve(listener, app).await.expect("serve");
}

async fn fetch(slots: &Semaphore, url: &str) -> Response {
    let Ok(_slot) = slots.acquire().await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match mailrs_fetch::fetch(url, mailrs_fetch::Limits::DEFAULT).await {
        Ok(image) => ([(header::CONTENT_TYPE, image.content_type)], image.bytes).into_response(),
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
