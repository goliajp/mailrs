//! Axum-based server scaffolding for the mailrs-core-api wire surface.
//!
//! Provides:
//! - `auth_middleware` — validates `Authorization: Bearer <secret>` header
//! - `base_router` — `/v1/healthz` + `/v1/readyz` mounted, ready for the
//!   backend (core or fastcore) to add its per-method routes
//!
//! Stub — full router + per-method handlers in subsequent loops
//! (checklist 1.13).

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
    routing::get,
};

use crate::method::ops::{OpsAuth, OpsStatusWire};
use crate::types::HealthResponse;

mod thread_events;
pub use thread_events::{ThreadEventHit, ThreadEventSink, thread_events};

/// Trait the backend implements to host a `mailrs-core-api` server.
///
/// `core` (PG-backed) and `fastcore` (KV-backed) each impl this with
/// their own concrete store inside.
///
/// Edition 2024 native async-in-trait — no `async-trait` dep needed.
pub trait Handler: Send + Sync + 'static {
    /// Reports backend kind + readiness for the /healthz probe.
    fn healthz(&self) -> impl std::future::Future<Output = HealthResponse> + Send;

    /// Deeper readiness probe — verifies the backend pool is open.
    fn readyz(&self) -> impl std::future::Future<Output = HealthResponse> + Send;

    /// What the ops daemon reads: drain progress and quarantine records.
    fn ops_status(&self) -> impl std::future::Future<Output = OpsStatusWire> + Send;
}

/// Build the base router: healthz/readyz (no auth) and the ops daemon's
/// status route (its own token, see [`crate::method::ops`]).
pub fn base_router<H: Handler>(handler: Arc<H>) -> Router {
    base_router_with_ops(handler, OpsAuth::from_env())
}

pub fn base_router_with_ops<H: Handler>(handler: Arc<H>, ops: OpsAuth) -> Router {
    let ops_route = Router::new()
        .route(
            crate::method::ops::PATH_OPS_STATUS,
            get(ops_status_handler::<H>),
        )
        .layer(axum::middleware::from_fn_with_state(
            Arc::new(ops),
            ops_auth_middleware,
        ))
        .with_state(handler.clone());
    Router::new()
        .route(
            crate::method::health::PATH_HEALTHZ,
            get(healthz_handler::<H>),
        )
        .route(crate::method::health::PATH_READYZ, get(readyz_handler::<H>))
        .with_state(handler)
        .merge(ops_route)
}

async fn ops_status_handler<H: Handler>(State(h): State<Arc<H>>) -> Json<OpsStatusWire> {
    Json(h.ops_status().await)
}

/// The ops daemon's bearer, compared by its sha256.
async fn ops_auth_middleware(
    State(auth): State<Arc<OpsAuth>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    use sha2::Digest;
    let bearer = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    let allowed = match (auth.as_ref(), bearer) {
        (OpsAuth::Open, _) => true,
        (OpsAuth::Closed, _) => false,
        (OpsAuth::TokenSha256(want), Some(token)) => {
            hex::encode(sha2::Sha256::digest(token.as_bytes())) == *want
        }
        (OpsAuth::TokenSha256(_), None) => false,
    };
    match allowed {
        true => Ok(next.run(req).await),
        false => Err(StatusCode::UNAUTHORIZED),
    }
}

async fn healthz_handler<H: Handler>(State(h): State<Arc<H>>) -> Json<HealthResponse> {
    Json(h.healthz().await)
}

async fn readyz_handler<H: Handler>(State(h): State<Arc<H>>) -> Json<HealthResponse> {
    Json(h.readyz().await)
}

/// Bearer-auth middleware to wrap authenticated route subtrees.
///
/// Caller passes the expected secret via `Arc<String>` (loaded from
/// `MAILRS_CORE_API_SECRET` env at boot).
pub async fn auth_middleware(
    State(expected_secret): State<Arc<String>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    match header {
        Some(token) if token == expected_secret.as_str() => Ok(next.run(req).await),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}
