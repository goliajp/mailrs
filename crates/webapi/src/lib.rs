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
pub mod session;

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
    /// HTTP client for the core/fastcore RPC. Used for everything
    /// EXCEPT the routes `fastcore_client` covers (when set).
    pub core_client: Arc<mailrs_core_api::client::Client>,
    /// Optional second client pointing at a kevy-backed fastcore.
    /// When `Some`, conversation list / thread mutations / counts /
    /// categories / per-thread messages are served via this client.
    /// Everything else (auth, admin, mail send, drafts, signatures,
    /// templates, queue) stays on `core_client`.
    ///
    /// Set via `MAILRS_FASTCORE_RPC_BASE` env var. When unset, all
    /// requests go to `core_client` and webapi behaves identically
    /// to the pre-fastcore build.
    pub fastcore_client: Option<Arc<mailrs_core_api::client::Client>>,
    /// Process bind address for the public REST/MCP listener.
    pub bind_addr: String,
    /// Shared WS broadcast bus, initialized lazily on the first
    /// `/api/events` upgrade. Held here so all WS clients share
    /// a single kevy subscribe loop.
    pub event_bus: std::sync::OnceLock<handlers::events::EventBus>,
}

impl WebState {
    /// Pick which client handles a "fastcore-eligible" route.
    /// Used by conversation list / thread mutation handlers.
    pub fn fast(&self) -> &Arc<mailrs_core_api::client::Client> {
        self.fastcore_client.as_ref().unwrap_or(&self.core_client)
    }
}

impl WebState {
    /// Build state from env. Panics if `MAILRS_CORE_API_SECRET` is missing.
    pub fn from_env() -> Self {
        let base = std::env::var("MAILRS_CORE_RPC_BASE")
            .unwrap_or_else(|_| "http://localhost:3300".into());
        let secret = std::env::var(mailrs_core_api::AUTH_SECRET_ENV)
            .expect("MAILRS_CORE_API_SECRET required for webapi");
        let core_client = Arc::new(mailrs_core_api::client::Client::new(base, secret.clone()));
        let fastcore_client = match std::env::var("MAILRS_FASTCORE_RPC_BASE") {
            Ok(fbase) if !fbase.is_empty() => {
                tracing::info!(
                    fastcore_base = %fbase,
                    "fastcore RPC enabled — conversation/thread reads go to fastcore"
                );
                Some(Arc::new(mailrs_core_api::client::Client::new(
                    fbase, secret,
                )))
            }
            _ => None,
        };
        let bind_addr = std::env::var("MAILRS_WEB_BIND").unwrap_or_else(|_| "0.0.0.0:3100".into());
        Self {
            core_client,
            fastcore_client,
            bind_addr,
            event_bus: std::sync::OnceLock::new(),
        }
    }
}

/// Build the axum router. Conversation routes wired (Phase 3.5);
/// auth + rest fill in next loops.
pub fn build_router(state: Arc<WebState>) -> axum::Router {
    use axum::routing::{get, post};
    use handlers::conversations as c;
    let _ = stub_auth_middleware; // kept for tests / dev mode reference

    use axum::routing::put;
    let convo = axum::Router::new()
        .route("/api/conversations", get(c::get_conversations))
        .route("/api/conversations/categories", get(c::get_categories))
        .route("/api/conversations/action-count", get(c::get_action_count))
        .route("/api/conversations/unseen-count", get(c::get_unseen_count))
        .route(
            "/api/conversations/{thread_id}/read",
            post(c::mark_thread_read),
        )
        .route(
            "/api/conversations/{thread_id}/unread",
            post(c::mark_thread_unread),
        )
        .route("/api/conversations/{thread_id}/star", post(c::star_thread))
        .route(
            "/api/conversations/{thread_id}/unstar",
            post(c::unstar_thread),
        )
        .route("/api/conversations/{thread_id}/pin", post(c::pin_thread))
        .route(
            "/api/conversations/{thread_id}/unpin",
            post(c::unpin_thread),
        )
        .route(
            "/api/conversations/{thread_id}/archive",
            post(c::archive_thread),
        )
        .route(
            "/api/conversations/{thread_id}/unarchive",
            post(c::unarchive_thread),
        )
        .route(
            "/api/conversations/{thread_id}/dismiss-action",
            post(c::dismiss_action),
        )
        .route(
            "/api/conversations/{thread_id}/snooze",
            put(c::snooze_thread).delete(c::unsnooze_thread),
        )
        .route(
            "/api/conversations/{thread_id}",
            get(c::get_thread_messages).delete(c::delete_thread),
        )
        .route(
            "/api/conversations/{thread_id}/reactions",
            get(handlers::mail::get_thread_reactions),
        )
        .route(
            "/api/conversations/{thread_id}/messages/{uid}/reactions",
            put(handlers::mail::toggle_reaction),
        );

    // Phase 12b — all /api/mail/* + BIMI + proxy routes are now
    // fastcore-native (kevy + maildir + external HTTP). Zero spg touch.
    use axum::routing::delete;
    let mail = axum::Router::new()
        .route("/api/mail/folders", get(handlers::mail::get_folders))
        .route(
            "/api/mail/messages/{uid}/raw",
            get(handlers::messages::get_message_raw),
        )
        .route(
            "/api/mail/messages/{uid}/attachments/{index}",
            get(handlers::messages::get_attachment),
        )
        .route(
            "/api/mail/messages/{uid}/attachments/{index}/content",
            get(handlers::messages::get_attachment_content),
        )
        .route(
            "/api/mail/messages/{uid}/flags",
            post(handlers::messages::update_flags),
        )
        .route(
            "/api/mail/inline-upload",
            post(handlers::inline::inline_upload),
        )
        .route("/api/mail/inline/{id}", get(handlers::inline::get_inline))
        .route(
            "/api/mail/keys",
            get(handlers::misc::get_keys).post(handlers::misc::save_key),
        )
        .route(
            "/api/mail/check-deliverability",
            get(handlers::misc::check_deliverability),
        )
        .route(
            "/api/mail/spam-feedback",
            post(handlers::misc::spam_feedback),
        )
        .route(
            "/api/mail/render-preview",
            post(handlers::misc::render_preview),
        )
        .route(
            "/api/mail/render-preview/cache/{id}",
            get(handlers::misc::render_preview_cached),
        )
        .route("/api/mail/export", get(handlers::misc::export_mbox))
        .route(
            "/api/conversations/search",
            get(handlers::misc::search_conversations),
        )
        .route("/api/mail/send", post(handlers::prefs::send_message))
        .route("/api/queue", get(handlers::prefs::get_queue_stats))
        .route("/api/contacts", get(handlers::prefs::get_contacts))
        .route("/api/mail/feedback", post(handlers::prefs::submit_feedback))
        .route(
            "/api/mail/drafts",
            get(handlers::prefs::list_drafts).post(handlers::prefs::save_draft),
        )
        .route(
            "/api/mail/drafts/{id}",
            delete(handlers::prefs::delete_draft),
        )
        .route(
            "/api/mail/signatures",
            get(handlers::prefs::list_signatures).post(handlers::prefs::save_signature),
        )
        .route(
            "/api/mail/signatures/{id}",
            delete(handlers::prefs::delete_signature),
        )
        .route(
            "/api/mail/templates",
            get(handlers::prefs::list_templates).post(handlers::prefs::save_template),
        )
        .route(
            "/api/mail/templates/{id}",
            delete(handlers::prefs::delete_template),
        )
        .route("/api/bimi/{domain}", get(handlers::prefs::get_bimi))
        .route("/api/proxy/image", get(handlers::prefs::proxy_image))
        .route("/api/proxy/link", get(handlers::prefs::proxy_link));

    let auth_routes = axum::Router::new()
        .route("/api/auth/me", get(handlers::auth::auth_me))
        .route("/api/auth/logout", post(handlers::auth::logout));

    let admin_routes = axum::Router::new()
        .route(
            "/api/admin/accounts",
            get(handlers::admin::list_accounts).post(handlers::admin::add_account),
        )
        .route(
            "/api/admin/accounts/{address}",
            delete(handlers::admin::remove_account),
        )
        .route(
            "/api/admin/aliases",
            get(handlers::admin::list_aliases).post(handlers::admin::add_alias),
        )
        .route(
            "/api/admin/aliases/{id}",
            delete(handlers::admin::remove_alias),
        )
        .route(
            "/api/admin/domains",
            get(handlers::admin::list_domains).post(handlers::admin::add_domain),
        )
        .route(
            "/api/admin/domains/{name}",
            delete(handlers::admin::remove_domain),
        )
        .route(
            "/api/admin/webhook-subscriptions",
            post(handlers::admin::create_webhook),
        )
        .route(
            "/api/admin/webhook-subscriptions/{id}",
            delete(handlers::admin::delete_webhook),
        )
        .route(
            "/api/admin/accounts/{address}/webhook-subscriptions",
            get(handlers::admin::list_webhooks),
        )
        .route("/api/admin/audit-log", get(handlers::admin::list_audit_log));

    // Phase 3.9 — real session auth via kevy when MAILRS_KEVY_URL is set;
    // falls back to the X-Mailrs-User header in dev (no kevy) mode.
    let authenticated = convo
        .merge(mail)
        .merge(auth_routes)
        .merge(admin_routes)
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            session::session_auth_middleware,
        ));

    // Unauthenticated routes — login + health. login intentionally
    // sits outside session_auth_middleware so a freshly-arrived client
    // (no session yet) can establish one.
    let unauth = axum::Router::new()
        .route("/_health", get(health_handler))
        .route("/api/health", get(health_handler))
        .route("/api/readiness", get(readiness_handler))
        .route("/api/status", get(status_handler))
        .route("/api/auth/login", post(handlers::auth::login))
        // WS upgrade uses `?token=<hex>` from query — browsers can't
        // set custom headers on WebSocket. Auth is inside the handler
        // (checks kevy `session:<token>` directly).
        .route("/api/events", get(handlers::events::ws_events));

    let mut app = unauth
        .merge(authenticated)
        .layer(
            tower_http::trace::TraceLayer::new_for_http()
                .make_span_with(
                    tower_http::trace::DefaultMakeSpan::new().level(tracing::Level::INFO),
                )
                .on_response(
                    tower_http::trace::DefaultOnResponse::new().level(tracing::Level::INFO),
                )
                .on_failure(tower_http::trace::DefaultOnFailure::new().level(tracing::Level::WARN)),
        )
        .with_state(state);

    // Serve the React UI from `MAILRS_WEB_STATIC_DIR` (defaults to
    // `/opt/mailrs/web` to match the monolith's bind-mount layout).
    // SPA fallback: any non-API path serves index.html so client-side
    // routing works.
    let static_dir =
        std::env::var("MAILRS_WEB_STATIC_DIR").unwrap_or_else(|_| "/opt/mailrs/web".to_string());
    if std::path::Path::new(&static_dir)
        .join("index.html")
        .exists()
    {
        use tower_http::services::{ServeDir, ServeFile};
        let index = format!("{static_dir}/index.html");
        app = app.fallback_service(ServeDir::new(&static_dir).fallback(ServeFile::new(index)));
        tracing::info!(dir = %static_dir, "webapi serving static UI");
    } else {
        tracing::info!(
            dir = %static_dir,
            "MAILRS_WEB_STATIC_DIR missing index.html — webapi will 404 non-API paths"
        );
    }
    app
}

async fn health_handler() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({"status": "healthy"}))
}

/// /api/readiness — deep probe: does core RPC answer?
async fn readiness_handler(
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
) -> Result<axum::Json<serde_json::Value>, axum::http::StatusCode> {
    match state.core_client.readyz().await {
        Ok(h) if h.ready => Ok(axum::Json(serde_json::json!({
            "status": "ready",
            "core_version": h.version,
        }))),
        Ok(_) => Err(axum::http::StatusCode::SERVICE_UNAVAILABLE),
        Err(_) => Err(axum::http::StatusCode::SERVICE_UNAVAILABLE),
    }
}

/// /api/status — version + build info. No auth required.
async fn status_handler() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "service": "mailrs-webapi",
    }))
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
        .insert(handlers::conversations::AuthedDisplayName::default());
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
        "mailrs-webapi starting"
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
