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

use crate::types::HealthResponse;

/// Trait the backend implements to host a `mailrs-core-api` server.
///
/// `core` (PG-backed) and `fastcore` (KV-backed) each impl this with
/// their own concrete store inside.
///
/// Edition 2024 native async-in-trait — no `async-trait` dep needed.
pub trait Handler: Send + Sync + 'static {
    /// Reports backend kind + readiness for the /healthz probe.
    fn healthz(&self) -> impl std::future::Future<Output = HealthResponse> + Send;

    /// Deeper readiness probe — verifies backend pool open + meili etc.
    fn readyz(&self) -> impl std::future::Future<Output = HealthResponse> + Send;
}

/// Build the base router with healthz/readyz mounted (no auth on health).
///
/// Per-method routes are added by the backend in later checklist items.
pub fn base_router<H: Handler>(handler: Arc<H>) -> Router {
    Router::new()
        .route(
            crate::method::health::PATH_HEALTHZ,
            get(healthz_handler::<H>),
        )
        .route(crate::method::health::PATH_READYZ, get(readyz_handler::<H>))
        .with_state(handler)
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
