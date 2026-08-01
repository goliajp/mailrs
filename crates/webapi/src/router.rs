//! The route table — every path the browser and the MCP clients can hit.
//!
//! 789 lines of `.route(...)` split out of `lib.rs` on 2026-08-02. It is
//! built at process start, so a path string that fails to parse panics
//! there rather than on the first request: `the_route_table_can_actually_be_built`
//! exists because 4,483 workspace tests were green while none of them
//! constructed it, and 2.19.0 shipped with the REST API in a restart loop.

use std::sync::Arc;

use crate::*;

mod mail;
mod rest;

use mail::*;
use rest::*;

/// Build the axum router. Conversation routes wired (Phase 3.5);
/// auth + rest fill in next loops.
pub fn build_router(state: Arc<WebState>) -> axum::Router {
    let _ = stub_auth_middleware; // kept for tests / dev mode reference

    let convo = conversation_routes();

    // Phase 12b — all /api/mail/* + BIMI + proxy routes are now
    // fastcore-native (kevy + maildir + external HTTP). Zero spg touch.
    let mail = mail_routes();

    let auth_routes = auth_routes();

    // JMAP endpoints (authenticated).
    let jmap_routes = jmap_routes();

    // DAV endpoints (authenticated). CalDAV / CardDAV clients drive
    // discovery with OPTIONS / PROPFIND / REPORT — axum's
    // MethodRouter only routes standard verbs, so collection routes
    // use `any(...)` so every method (including PROPFIND / REPORT /
    // MKCALENDAR) lands on the same handler which inspects the
    // Method header. Leaf item routes stick to PUT/GET/DELETE.
    let dav_routes = dav_routes();

    let admin_routes = admin_routes();

    // Phase 3.9 — real session auth via kevy when MAILRS_KEVY_URL is set;
    // falls back to the X-Mailrs-User header in dev (no kevy) mode.
    let authenticated = convo
        .merge(mail)
        .merge(auth_routes)
        .merge(admin_routes)
        .merge(jmap_routes)
        .merge(dav_routes)
        // admin gate — 403 unless caller has admin.* permission or is_super.
        // Runs after session_auth (below) so authed user is in Extensions.
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            handlers::kevy_util::admin_middleware,
        ))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            session::session_auth_middleware,
        ));

    // Unauthenticated routes — login + health. login intentionally
    // sits outside session_auth_middleware so a freshly-arrived client
    // (no session yet) can establish one.
    let unauth = unauth_routes();

    // Match monolith's 25 MiB multipart cap. Axum's default is 2 MiB,
    // which trips /api/mail/send-multipart on any attached file bigger
    // than that — the UI just shows "Send failed" with no server-side
    // trace. See crates/server/src/web/mod.rs:MAX_MULTIPART_BODY.
    const MAX_MULTIPART_BODY: usize = 25 * 1024 * 1024;

    // MCP Streamable HTTP surface at /mcp. Runs its own auth
    // middleware (task-local user) so rmcp's session factory sees
    // the caller. Mounted OUTSIDE the REST auth stack because rmcp
    // manages its own Extension shape.
    let mcp = handlers::mcp::mcp_router(state.clone()).route_layer(
        axum::middleware::from_fn_with_state(state.clone(), handlers::mcp::mcp_auth_middleware),
    );

    let mut app = unauth
        .merge(authenticated)
        .merge(mcp)
        .layer(axum::extract::DefaultBodyLimit::max(MAX_MULTIPART_BODY))
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
        // `/assets/*` is served without the SPA fallback. Those filenames
        // carry a content hash, so a miss means the file is genuinely gone —
        // and answering it with index.html means a 200 whose body is HTML
        // where the browser expected a JS module. It then fails to parse,
        // which surfaces as a lazy-import rejection and the route error
        // boundary: "Something went wrong", with nothing pointing at the
        // real cause. A 404 says what happened (2026-07-30).
        let assets = format!("{static_dir}/assets");
        app = app
            .nest_service("/assets", ServeDir::new(&assets))
            .fallback_service(ServeDir::new(&static_dir).fallback(ServeFile::new(index)));
        tracing::info!(dir = %static_dir, "webapi serving static UI");
    } else {
        tracing::info!(
            dir = %static_dir,
            "MAILRS_WEB_STATIC_DIR missing index.html — webapi will 404 non-API paths"
        );
    }
    app
}
