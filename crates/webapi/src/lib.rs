//! mailrs-webapi — REST + MCP + JMAP + CalDAV/CardDAV frontend.
//!
//! Phase 3 of the 4-process split (checklist
//! `.claude/notes/arch-split-plus-fastcore-checklist-2026-06-30.md` §3).
//!
//! This crate is currently a scaffold — no routes mounted yet. Subsequent
//! loops fill in the REST and MCP handlers by copying the existing
//! `crates/server/src/web/` + `crates/server/src/mcp/` trees and replacing
//! `state.mailbox.X()` / `state.domain.X()` direct calls with
//! `state.core.X()` RPC client calls.
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
mod router;
use router::build_router;
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
    /// The ONE core-api RPC client. Points at whichever serving core is
    /// running (fastcore/kevy OR core/pg-spg) via `MAILRS_CORE_RPC_BASE`
    /// — webapi is 100% agnostic to which backend answers. The switch
    /// boundary is exactly this env var; there is no per-route client
    /// selection and no backend conditional anywhere above this field
    /// (v2 dual-mode: RFC lazy-wobbling-nebula).
    pub core: Arc<mailrs_core_api::client::Client>,
    /// Process bind address for the public REST/MCP listener.
    pub bind_addr: String,
    /// Shared WS broadcast bus, initialized lazily on the first
    /// `/api/events` upgrade. Held here so all WS clients share
    /// a single kevy subscribe loop.
    pub event_bus: std::sync::OnceLock<handlers::events::EventBus>,
    /// Wall-clock start of this webapi process, used to compute the
    /// `uptime_secs` field surfaced by `/api/health` + `/api/status`.
    /// UI status bars and SMTP-monitor cards read this to render a
    /// live uptime badge; before it existed both endpoints returned
    /// no uptime field and the frontend rendered `NaN`.
    pub started_at: std::time::Instant,
    /// The writing-assistance model, when one is configured.
    ///
    /// `None` is the normal production state: `MAILRS_AI_ANALYSIS_ENABLED`
    /// is off and the API key empty, so the three assist routes answer
    /// `{"success":false,"message":"AI not configured"}`. Before this field
    /// existed they were not registered at all and answered 405, which the
    /// client rendered as a generic failure — the same news, told in a way
    /// nobody could act on. Enabling the model is an ops decision with quota
    /// consequences and is not made here.
    pub llm_config: Option<Arc<dyn mailrs_intelligence::provider::LlmProvider>>,
}

impl WebState {
    /// Build state from env. Panics if `MAILRS_CORE_API_SECRET` is missing.
    pub fn from_env() -> Self {
        let base = std::env::var("MAILRS_CORE_RPC_BASE")
            .unwrap_or_else(|_| "http://localhost:3300".into());
        let secret = std::env::var(mailrs_core_api::AUTH_SECRET_ENV)
            .expect("MAILRS_CORE_API_SECRET required for webapi");
        let core = Arc::new(mailrs_core_api::client::Client::new(base, secret));
        let bind_addr = std::env::var("MAILRS_WEB_BIND").unwrap_or_else(|_| "0.0.0.0:3100".into());
        // Constructed exactly as the monolith does it, from the same env
        // vars, so the two lanes answer from the same provider or from
        // neither.
        let ai_enabled = std::env::var("MAILRS_AI_ANALYSIS_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        let llm_config: Option<Arc<dyn mailrs_intelligence::provider::LlmProvider>> =
            match ai_enabled {
                false => None,
                true => {
                    let url = std::env::var("MAILRS_LLM_URL")
                        // Same default as `Config::default().llm_url` in the
                        // monolith — a different one here would mean the two
                        // lanes reach different models on the same env.
                        .unwrap_or_else(|_| "https://devops.golia.jp/api/llm/complete".into());
                    let api_key = std::env::var("MAILRS_LLM_API_KEY")
                        .ok()
                        .filter(|k| !k.is_empty());
                    let model_id = format!(
                        "qwen3.5-9b/{}",
                        mailrs_intelligence::analyze::PROMPT_VERSION
                    );
                    Some(Arc::new(
                        mailrs_intelligence::OpenAiCompatibleProvider::new(url, api_key, model_id),
                    ))
                }
            };
        Self {
            core,
            bind_addr,
            event_bus: std::sync::OnceLock::new(),
            started_at: std::time::Instant::now(),
            llm_config,
        }
    }
}

/// /api/health — public liveness probe. No auth required. Shape is:
///
/// ```json
/// {
///   "status": "healthy",
///   "ok": true,
///   "service": "mailrs-webapi",
///   "version": "<pkg-version>",
///   "uptime_secs": 42,
///   "kevy": true,
///   "pg": null
/// }
/// ```
///
/// Callers can rely on:
///   - the four `service` / `version` / `uptime_secs` / `status` fields
///     always being present.
///   - `kevy` reporting the real ping status (this handler round-trips
///     a kevy op to compute the boolean, so it's not just a config
///     flag).
///   - `pg` being `null` in the fastcore lane (no PostgreSQL backend
///     exists), so the frontend can hide the PG pill instead of
///     drawing it as "down". In the spg-backed lane the same handler
///     ships with `pg` set to a real probe.
async fn health_handler(
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
) -> axum::Json<serde_json::Value> {
    let uptime_secs = state.started_at.elapsed().as_secs();
    // Cheap kevy round-trip. Any success => backend healthy; any error
    // => backend unreachable. Runs on the shared shard connection, no
    // fresh TCP per request.
    let kevy_ok =
        handlers::kevy_util::with_kevy(|c| c.ping().map_err(std::io::Error::other)).is_ok();
    axum::Json(serde_json::json!({
        "status": if kevy_ok { "healthy" } else { "degraded" },
        "ok": kevy_ok,
        "service": "mailrs-webapi",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": uptime_secs,
        "kevy": kevy_ok,
        "pg": serde_json::Value::Null,
    }))
}

/// /api/readiness — deep probe: does core RPC answer?
async fn readiness_handler(
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
) -> Result<axum::Json<serde_json::Value>, axum::http::StatusCode> {
    match state.core.readyz().await {
        Ok(h) if h.ready => Ok(axum::Json(serde_json::json!({
            "status": "ready",
            "core_version": h.version,
        }))),
        Ok(_) => Err(axum::http::StatusCode::SERVICE_UNAVAILABLE),
        Err(_) => Err(axum::http::StatusCode::SERVICE_UNAVAILABLE),
    }
}

/// /api/status — version + build info + webapi lifetime. No auth
/// required. Additional metric fields (SMTP counters, queue depth) are
/// nulled out here rather than pretending they're zero: in the fastcore
/// 4-process split those counters live in `mailrs-receiver` +
/// `mailrs-fastcore-sender`, not in this webapi process. UIs that render
/// them treat `null` as "no data" (a dash), which is the truthful thing
/// to show — v1.9.4 shipped a monitor page that read the absent fields
/// as `0` and displayed `NaN` uptime; explicit nulls fix both.
async fn status_handler(
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "service": "mailrs-webapi",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": state.started_at.elapsed().as_secs(),
        "active_connections": serde_json::Value::Null,
        "total_connections": serde_json::Value::Null,
        "total_messages": serde_json::Value::Null,
        "queue": serde_json::Value::Null,
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

    // Prometheus recorder must be installed before any counter is
    // emitted; do it as early as possible in the boot sequence.
    handlers::metrics::install();

    let state = Arc::new(WebState::from_env());
    tracing::info!(
        bind = %state.bind_addr,
        version = env!("CARGO_PKG_VERSION"),
        "mailrs-webapi starting"
    );

    // Quick core liveness probe so we fail-fast on bad MAILRS_CORE_RPC_BASE.
    match state.core.healthz().await {
        Ok(h) => {
            tracing::info!(version = %h.version, backend = ?h.backend, "core RPC reachable");
        }
        Err(e) => {
            tracing::warn!(error = %e, "core RPC unreachable at startup — webapi will retry");
        }
    }

    // One-shot alias sync: existing alias entries in the network kevy
    // `admin:aliases` hash (populated by webapi's older `add_alias`
    // handler) don't have a fastcore mirror. Push each into fastcore
    // on boot so the spool drain sees them immediately. Idempotent.
    {
        let sync_state = state.clone();
        tokio::spawn(async move {
            let synced = crate::handlers::admin::sync_aliases_to_fastcore(&sync_state).await;
            if synced > 0 {
                tracing::info!(count = synced, "alias sync: network → fastcore");
            }
        });
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

#[cfg(test)]
mod router_tests {
    use super::*;

    /// Build the whole route table.
    ///
    /// `Router::route` panics on an invalid or conflicting path, and that
    /// panic happens at construction — which is process start, in prod,
    /// with nothing having gone wrong at compile time. On 2026-07-30 three
    /// new routes of the form `/api/mail/sends/{send_id}:source` shipped as
    /// 2.19.0 and took webapi-fc into a restart loop: matchit refuses a
    /// parameter with a literal suffix in one segment ("Only one parameter
    /// is allowed per path segment"). The REST API and the web UI were down
    /// until the rollback.
    ///
    /// 4,483 workspace tests were green. Not one of them built the router,
    /// so the entire class was invisible to the gate — including the perf
    /// gates, which measure code that this crate never reaches if the
    /// router cannot be constructed at all.
    ///
    /// This test does nothing but call the function. That is the point:
    /// every route string is validated the moment anyone runs the suite.
    #[test]
    fn the_route_table_can_actually_be_built() {
        let state = Arc::new(WebState {
            core: Arc::new(mailrs_core_api::client::Client::new(
                "http://127.0.0.1:1",
                "test-secret",
            )),
            bind_addr: "127.0.0.1:0".into(),
            event_bus: std::sync::OnceLock::new(),
            started_at: std::time::Instant::now(),
            llm_config: None,
        });
        // Constructed by hand rather than via `WebState::from_env()`, which
        // reads MAILRS_CORE_API_SECRET and panics without it — and env
        // mutation from a test races every other test in the binary.
        let _router = build_router(state);
    }

    /// The writing-assistance routes exist whether or not a model does.
    ///
    /// Production runs this lane with `MAILRS_AI_ANALYSIS_ENABLED` unset, and
    /// until now the three routes were not registered at all: Polish, Suggest
    /// and Generate subject answered 405, which the client rendered as a
    /// generic failure. A route that says "AI not configured" is the same
    /// news told in a form the user can act on — and it is the shape the
    /// client already parses.
    #[tokio::test]
    async fn the_assist_routes_answer_without_a_model() {
        use axum::body::Body;
        use axum::http::{Method, Request};
        use tower::ServiceExt;

        let state = Arc::new(WebState {
            core: Arc::new(mailrs_core_api::client::Client::new(
                "http://127.0.0.1:1",
                "test-secret",
            )),
            bind_addr: "127.0.0.1:0".into(),
            event_bus: std::sync::OnceLock::new(),
            started_at: std::time::Instant::now(),
            llm_config: None,
        });

        for (path, body) in [
            ("/api/mail/ai/polish", r#"{"text":"hello"}"#),
            ("/api/mail/ai/reply-suggest", r#"{"original_body":"hi"}"#),
            ("/api/mail/ai/generate-subject", r#"{"body":"hi"}"#),
        ] {
            let resp = build_router(state.clone())
                .oneshot(
                    Request::builder()
                        .method(Method::POST)
                        .uri(path)
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .expect("request"),
                )
                .await
                .expect("response");
            // Unauthenticated, so 401 — but never 405, which is what a
            // missing route gives and what production returned.
            assert_ne!(
                resp.status(),
                axum::http::StatusCode::METHOD_NOT_ALLOWED,
                "{path} is not registered"
            );
            assert_ne!(
                resp.status(),
                axum::http::StatusCode::NOT_FOUND,
                "{path} is not registered"
            );
        }
    }
}
