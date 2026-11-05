//! The daemon's own HTTP surface.
//!
//! On its network port: `POST /v1/reports` from the processes, and the
//! administrator routes for webapi, which proves itself with the shared
//! secret (`x-mailrs-ops-admin`, compared by sha256 with
//! `MAILRS_OPS_ADMIN_SECRET_SHA256`) and names the administrator in
//! `x-mailrs-ops-actor`. On the Unix socket in the daemon's data
//! directory: the same administrator routes for `mailrs-ops` inside the
//! container, where reaching the socket is the proof.

mod admin;
mod reports;

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::{delete, get, post};

use crate::app::App;

pub const ADMIN_SECRET_HEADER: &str = "x-mailrs-ops-admin";
pub const ACTOR_HEADER: &str = "x-mailrs-ops-actor";

/// Who is calling an administrator route.
#[derive(Debug, Clone)]
pub enum Caller {
    /// webapi, on behalf of the administrator it names.
    Webapi { actor: Option<String> },
    /// `mailrs-ops` over the socket; the person is named with `--by`.
    Socket,
}

fn admin_routes(app: Arc<App>) -> Router<()> {
    Router::new()
        .route("/v1/state", get(admin::state))
        .route("/v1/history", get(admin::history))
        .route("/v1/silences", post(admin::set_silence))
        .route("/v1/silences/{id}", delete(admin::end_silence))
        .route("/v1/maintenance", post(admin::maintenance))
        .route("/v1/channels/test", post(admin::test_channels))
        .with_state(app)
}

/// The network port's routes.
pub fn tcp_router(app: Arc<App>) -> Router {
    let admin = admin_routes(app.clone()).layer(axum::middleware::from_fn_with_state(
        app.clone(),
        webapi_auth,
    ));
    Router::new()
        .route(
            mailrs_ops_report::wire::REPORTS_PATH,
            post(reports::receive),
        )
        .layer(axum::extract::DefaultBodyLimit::max(
            mailrs_ops_report::wire::MAX_REPORT_BYTES,
        ))
        .with_state(app)
        .merge(admin)
}

/// The socket's routes.
pub fn socket_router(app: Arc<App>) -> Router {
    admin_routes(app).layer(axum::middleware::from_fn(
        |mut req: Request, next: Next| async move {
            req.extensions_mut().insert(Caller::Socket);
            next.run(req).await
        },
    ))
}

async fn webapi_auth(
    State(app): State<Arc<App>>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    use sha2::Digest;
    let Some(want) = &app.cfg.admin_secret_sha256 else {
        return Err(StatusCode::FORBIDDEN);
    };
    let given = req
        .headers()
        .get(ADMIN_SECRET_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if hex::encode(sha2::Sha256::digest(given.as_bytes())) != *want {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let actor = req
        .headers()
        .get(ACTOR_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .filter(|a| !a.trim().is_empty());
    req.extensions_mut().insert(Caller::Webapi { actor });
    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests;
