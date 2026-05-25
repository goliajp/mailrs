//! Web router — assembles the axum `Router` from per-area route
//! builders + middleware stack (CORS, security headers, rate
//! limit, request id, body limit). Each builder fn returns a
//! `Router<Arc<WebState>>` for one logical API surface.

use std::sync::Arc;
use std::time::Duration;

use axum::middleware;
use axum::routing::{get, post};
use tower_http::cors::CorsLayer;

use super::{MAX_MULTIPART_BODY, WebState, auth, mail, rate_limit, request_id};

/// middleware that adds security headers to all responses
async fn security_headers(
    request: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::X_CONTENT_TYPE_OPTIONS,
        "nosniff".parse().unwrap(),
    );
    headers.insert(axum::http::header::X_FRAME_OPTIONS, "DENY".parse().unwrap());
    headers.insert(axum::http::header::X_XSS_PROTECTION, "0".parse().unwrap());
    headers.insert(
        axum::http::header::REFERRER_POLICY,
        "strict-origin-when-cross-origin".parse().unwrap(),
    );
    headers.insert(
        axum::http::HeaderName::from_static("permissions-policy"),
        "camera=(), microphone=(), geolocation=()".parse().unwrap(),
    );
    // CSP: strict script-src, inline styles for tailwind, data: images for
    // embedded email content, websocket via connect-src, srcdoc iframes via
    // frame-src 'self', and base-uri/form-action lockdown
    headers.insert(
        axum::http::header::CONTENT_SECURITY_POLICY,
        concat!(
            "default-src 'self'; ",
            "script-src 'self'; ",
            "style-src 'self' 'unsafe-inline'; ",
            "img-src 'self' data:; ",
            "font-src 'self'; ",
            "connect-src 'self'; ",
            "frame-src 'self'; ",
            "base-uri 'self'; ",
            "form-action 'self'",
        )
        .parse()
        .unwrap(),
    );
    response
}

pub fn router(state: Arc<WebState>, static_dir: Option<&str>) -> axum::Router {
    let rate_limiter = state.web_rate_limiter.clone();

    // mcp router: auth middleware but no general rate limiter (MCP sessions are long-lived)
    let mcp_router = crate::mcp::setup_mcp(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::mcp::auth::mcp_auth_middleware,
        ))
        .layer(middleware::from_fn(security_headers))
        .layer(
            CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::DELETE,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers([
                    axum::http::header::AUTHORIZATION,
                    axum::http::header::CONTENT_TYPE,
                ])
                .max_age(Duration::from_secs(3600)),
        );

    // auth routes with stricter rate limit (10 req/min per IP)
    let auth_routes = axum::Router::new()
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/forgot-password", post(auth::forgot_password))
        .route("/api/auth/reset-password", post(auth::reset_password))
        .route_layer(middleware::from_fn_with_state(
            rate_limiter.clone(),
            rate_limit::auth_rate_limit,
        ));

    let mut app = axum::Router::new()
        .merge(core_routes())
        .merge(mail_routes())
        .merge(conversations_routes())
        .merge(auth_routes)
        .merge(account_routes())
        .merge(agent_routes())
        .merge(admin_routes())
        .merge(protocol_routes())
        .merge(dav_routes())
        .layer(axum::extract::DefaultBodyLimit::max(MAX_MULTIPART_BODY))
        .layer(middleware::from_fn(request_id::request_id_middleware))
        .layer(middleware::from_fn_with_state(
            rate_limiter,
            rate_limit::general_rate_limit,
        ))
        .layer(middleware::from_fn(security_headers))
        .layer(
            CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::PUT,
                    axum::http::Method::PATCH,
                    axum::http::Method::DELETE,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers([
                    axum::http::header::AUTHORIZATION,
                    axum::http::header::CONTENT_TYPE,
                    axum::http::HeaderName::from_static("x-request-id"),
                ])
                .expose_headers([axum::http::HeaderName::from_static("x-request-id")])
                .max_age(Duration::from_secs(3600)),
        )
        .with_state(state.clone());

    // merge MCP router after with_state so it bypasses the general rate limiter
    app = app.merge(mcp_router.with_state(state.clone()));

    // BIMI logo lookup — bypasses rate limiter (cached DNS, read-only)
    // Image proxy — fetches external email images through our server (requires auth)
    let bimi_router = axum::Router::new()
        .route("/api/bimi/{domain}", get(mail::get_bimi_logo))
        .route("/api/proxy/image", get(mail::proxy_image))
        .route("/api/proxy/link", get(mail::proxy_link))
        .with_state(state);
    app = app.merge(bimi_router);

    // serve frontend static files with SPA fallback
    if let Some(dir) = static_dir {
        use tower_http::services::{ServeDir, ServeFile};
        let index = format!("{dir}/index.html");
        app = app.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)));
    }

    // HTTP-request-level tracing span. Wraps EVERY route (including the
    // post-with_state merges above) so all per-handler log lines + any
    // future #[instrument] handlers nest under one `web.req` span per
    // request. Span carries method + URI; status code + latency are added
    // on response.
    app = app.layer(
        tower_http::trace::TraceLayer::new_for_http().make_span_with(
            |req: &axum::http::Request<_>| {
                tracing::info_span!(
                    "web.req",
                    method = %req.method(),
                    uri = %req.uri(),
                )
            },
        ),
    );

    app
}

mod routes;
use routes::*;
