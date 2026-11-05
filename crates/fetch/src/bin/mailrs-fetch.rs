//! `mailrs-fetch` — fetches one remote image per request for fastcore.
//!
//! `POST /fetch {"url": …}` answers the re-encoded image, or 422 with why
//! it was refused. It holds no secret and reaches nothing of mailrs: it
//! sits on a network shared with fastcore alone, and its own checks refuse
//! every private address, that network's included.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Json;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use mailrs_ops_report::wire::{Detail, FetchBeat};
use tokio::sync::Semaphore;

/// Fetches in flight at once; the rest wait. Each decode may take up to
/// 128 MB, and the container has 512 MB.
const CONCURRENT: usize = 2;

#[derive(serde::Deserialize)]
struct FetchRequest {
    url: String,
}

/// Fetches since start, for the ops heartbeat.
static FETCHED: AtomicU64 = AtomicU64::new(0);
static REFUSED: AtomicU64 = AtomicU64::new(0);
static IN_FLIGHT: AtomicU64 = AtomicU64::new(0);

#[tokio::main]
async fn main() {
    let ops = mailrs_ops_report::push::Reporter::start("fetch");
    let bind = std::env::var("MAILRS_FETCH_BIND").unwrap_or_else(|_| "0.0.0.0:3500".into());
    let slots = Arc::new(Semaphore::new(CONCURRENT));
    let app = axum::Router::new()
        .route(
            "/fetch",
            post(move |Json(req): Json<FetchRequest>| {
                let slots = slots.clone();
                async move { fetch(&slots, &req.url).await }
            }),
        )
        // the container healthcheck
        .route("/healthz", get(|| async { StatusCode::NO_CONTENT }));
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .unwrap_or_else(|e| panic!("bind {bind}: {e}"));
    eprintln!("mailrs-fetch listening on {bind}");
    ops.ready(None);
    ops.spawn_heartbeat(std::time::Duration::from_secs(60), || {
        Some(Detail::Fetch(FetchBeat {
            fetched: FETCHED.load(Ordering::Relaxed),
            refused: REFUSED.load(Ordering::Relaxed),
            in_flight: IN_FLIGHT.load(Ordering::Relaxed),
        }))
    });
    axum::serve(listener, app).await.expect("serve");
}

async fn fetch(slots: &Semaphore, url: &str) -> Response {
    let Ok(_slot) = slots.acquire().await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    IN_FLIGHT.fetch_add(1, Ordering::Relaxed);
    let got = mailrs_fetch::fetch(url, mailrs_fetch::Limits::DEFAULT).await;
    IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
    match got {
        Ok(image) => {
            FETCHED.fetch_add(1, Ordering::Relaxed);
            ([(header::CONTENT_TYPE, image.content_type)], image.bytes).into_response()
        }
        Err(e) => {
            REFUSED.fetch_add(1, Ordering::Relaxed);
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    }
}
