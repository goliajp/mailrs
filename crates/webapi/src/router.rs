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
mod mock;
mod rest;

use mail::*;
use mock::*;
use rest::*;

/// Build the axum router. Conversation routes wired (Phase 3.5);
/// auth + rest fill in next loops.
pub fn build_router(state: Arc<WebState>) -> axum::Router {
    let convo = conversation_routes();

    // Phase 12b — all /api/mail/* + BIMI + proxy routes are now
    // fastcore-native (kevy + maildir + external HTTP). Zero spg touch.
    let mail = mail_routes();

    let auth_routes = auth_routes();

    // JMAP endpoints (authenticated).
    let jmap_routes = jmap_routes();

    // DAV endpoints: a session, a bearer token, or HTTP Basic, which is
    // the only login calendar clients speak.
    let dav = dav_routes().route_layer(axum::middleware::from_fn_with_state(
        state.clone(),
        handlers::dav_auth::dav_auth_middleware,
    ));

    let admin_routes = admin_routes();

    // session, bearer or agent key, resolved against the network kevy;
    // without one configured nothing authenticates
    let authenticated = convo
        .merge(mail)
        .merge(auth_routes)
        .merge(admin_routes)
        .merge(jmap_routes)
        .merge(mock_routes())
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

    // MCP Streamable HTTP surface at /mcp. Runs its own auth
    // middleware (task-local user) so rmcp's session factory sees
    // the caller. Mounted OUTSIDE the REST auth stack because rmcp
    // manages its own Extension shape.
    let mcp = handlers::mcp::mcp_router(state.clone()).route_layer(
        axum::middleware::from_fn_with_state(state.clone(), handlers::mcp::mcp_auth_middleware),
    );

    let mut app = unauth
        .merge(authenticated)
        .merge(dav)
        .merge(mcp)
        // an unknown API path is a 404, not the web app's index.html with a
        // 200: a client calling a wrong path otherwise parses HTML and the
        // error it reports names neither the path nor the cause
        .route("/api/{*rest}", axum::routing::any(api_not_found))
        .layer(axum::extract::DefaultBodyLimit::max(
            crate::handlers::send_error::MAX_MULTIPART_BODY,
        ))
        .layer(
            tower_http::trace::TraceLayer::new_for_http()
                // the path only: browsers can only send a WebSocket's or an
                // attachment link's session token in the query, and the
                // default span wrote it into the access log in clear
                .make_span_with(|req: &axum::http::Request<axum::body::Body>| {
                    tracing::info_span!("request", method = %req.method(), path = %loggable_path(req.uri().path()))
                })
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
    // a page's address never leaves for another site with a request it makes
    // or a link it follows: mail pages name threads, and links inside mail
    // lead anywhere. Not `no-referrer`: under it a browser sends `Origin:
    // null` on our own POSTs, which the login and CSRF origin checks refuse

    app.layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
        axum::http::header::REFERRER_POLICY,
        axum::http::HeaderValue::from_static("same-origin"),
    ))
}

async fn api_not_found(uri: axum::http::Uri) -> impl axum::response::IntoResponse {
    (
        axum::http::StatusCode::NOT_FOUND,
        axum::Json(serde_json::json!({ "error": format!("no API route {}", uri.path()) })),
    )
}

/// The path as the access log writes it. An inline image is served to
/// whoever holds its URL, so its id is the credential and stays out of
/// the log.
fn loggable_path(path: &str) -> &str {
    if path.starts_with("/api/mail/inline/") {
        return "/api/mail/inline/{id}";
    }
    path
}
