//! mailrs-webapi — REST + MCP + JMAP + CalDAV/CardDAV frontend.
//!
//! Phase 3 of the 4-process split (checklist
//! `.claude/notes/arch-split-plus-fastcore-checklist-2026-06-30.md` §3).
//!
//! This crate is currently a scaffold — no routes mounted yet. Subsequent
//! loops fill in the REST and MCP handlers by copying the existing
//! `crates/server/src/web/` + `crates/server/src/mcp/` trees and replacing
//! `state.mailbox.X()` / `state.domain.X()` direct calls with
//! `state.core_client.X()` RPC client calls.
//!
//! Boot order:
//! 1. tracing init
//! 2. config from env (MAILRS_CORE_RPC_BASE / MAILRS_CORE_API_SECRET /
//!    MAILRS_KEVY_URL / MAILRS_WEB_BIND etc.)
//! 3. mailrs-core-api client
//! 4. kevy_net client (for session store + cache bust)
//! 5. meili client (for search)
//! 6. axum router + listen
//! 7. signal handler

#![allow(missing_docs)]

pub mod handlers;

use std::sync::Arc;

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

/// Shared state injected into every web handler.
///
/// Distinct from the old `crate::server::web::WebState` — fewer fields
/// because PG/mailbox/domain backings now sit behind `core_client`.
pub struct WebState {
    /// HTTP client for the core/fastcore RPC.
    pub core_client: Arc<mailrs_core_api::client::Client>,
    /// Process bind address for the public REST/MCP listener.
    pub bind_addr: String,
}

impl WebState {
    /// Build state from env. Panics if `MAILRS_CORE_API_SECRET` is missing.
    pub fn from_env() -> Self {
        let base = std::env::var("MAILRS_CORE_RPC_BASE")
            .unwrap_or_else(|_| "http://localhost:3300".into());
        let secret = std::env::var(mailrs_core_api::AUTH_SECRET_ENV)
            .expect("MAILRS_CORE_API_SECRET required for webapi");
        let core_client = Arc::new(mailrs_core_api::client::Client::new(base, secret));
        let bind_addr = std::env::var("MAILRS_WEB_BIND").unwrap_or_else(|_| "0.0.0.0:3100".into());
        Self {
            core_client,
            bind_addr,
        }
    }
}

/// Build the axum router. Conversation routes wired (Phase 3.5);
/// auth + rest fill in next loops.
pub fn build_router(state: Arc<WebState>) -> axum::Router {
    use axum::routing::{get, post};
    use handlers::conversations as c;

    let convo = axum::Router::new()
        .route("/api/conversations", get(c::get_conversations))
        .route("/api/conversations/categories", get(c::get_categories))
        .route("/api/conversations/action-count", get(c::get_action_count))
        .route(
            "/api/conversations/{thread_id}/read",
            post(c::mark_thread_read),
        )
        .route("/api/conversations/{thread_id}/star", post(c::star_thread))
        .route(
            "/api/conversations/{thread_id}/archive",
            post(c::archive_thread),
        );

    let mail = axum::Router::new()
        .route("/api/mail/folders", get(handlers::mail::get_folders))
        .route("/api/mail/messages/{uid}", get(handlers::mail::get_message))
        .route("/api/mail/stats", get(handlers::mail::get_mail_stats));

    let authenticated = convo
        .merge(mail)
        .route_layer(axum::middleware::from_fn(stub_auth_middleware));

    axum::Router::new()
        .route("/_health", get(health_handler))
        .merge(authenticated)
        .with_state(state)
}

async fn health_handler() -> &'static str {
    "ok"
}

/// Phase 3 stub auth middleware — extracts user from `X-Mailrs-User`
/// header. Real session/JWT/api-key resolution lands in checklist 3.9.
async fn stub_auth_middleware(mut req: Request, next: Next) -> Result<Response, StatusCode> {
    let user = req
        .headers()
        .get("X-Mailrs-User")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    req.extensions_mut()
        .insert(handlers::conversations::AuthedUser(user));
    Ok(next.run(req).await)
}

/// Main entry — boots state, builds router, listens, handles shutdown.
pub async fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    let state = Arc::new(WebState::from_env());
    tracing::info!(
        bind = %state.bind_addr,
        version = env!("CARGO_PKG_VERSION"),
        "mailrs-webapi starting (Phase 3 scaffold — routes pending)"
    );

    // Quick core liveness probe so we fail-fast on bad MAILRS_CORE_RPC_BASE.
    match state.core_client.healthz().await {
        Ok(h) => {
            tracing::info!(version = %h.version, backend = ?h.backend, "core RPC reachable");
        }
        Err(e) => {
            tracing::warn!(error = %e, "core RPC unreachable at startup — webapi will retry");
        }
    }

    let bind = state.bind_addr.clone();
    let router = build_router(state);
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .unwrap_or_else(|e| panic!("webapi bind {bind} failed: {e}"));
    tracing::info!(addr = %bind, "webapi listening");

    let (_shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let server = axum::serve(listener, router).with_graceful_shutdown(async move {
        let _ = shutdown_rx.changed().await;
    });

    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! {
            r = tokio::signal::ctrl_c() => r.expect("ctrl_c"),
            _ = sigterm.recv() => {}
            r = server => { if let Err(e) = r { tracing::error!(error = %e, "webapi server exited"); } return; }
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await.expect("ctrl_c");

    tracing::info!("mailrs-webapi shutting down");
    let _ = _shutdown_tx.send(true);
}
