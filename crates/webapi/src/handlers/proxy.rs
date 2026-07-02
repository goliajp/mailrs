//! Transparent HTTP proxy to monolith's `:3100` web server.
//!
//! Phase 12 — for /api/mail/*, /api/bimi/*, /api/proxy/*, we don't
//! have kevy-native handlers yet. Rather than re-implement the wire
//! shape in webapi (dozens of tightly-coupled fields on the UI side),
//! forward the request verbatim to monolith which already produces
//! the exact shape the React UI expects.
//!
//! Env: `MAILRS_MONOLITH_WEB_BASE` — the monolith web endpoint
//! (default `http://mailrs:3100`).

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::Response;

use crate::WebState;

pub async fn proxy_to_monolith(
    State(state): State<Arc<WebState>>,
    req: Request,
) -> Result<Response, StatusCode> {
    let _ = state;
    let base = std::env::var("MAILRS_MONOLITH_WEB_BASE")
        .unwrap_or_else(|_| "http://mailrs:3100".to_string());

    let (parts, body) = req.into_parts();
    let uri = parts.uri.clone();
    let path_and_query = uri.path_and_query().map(|p| p.as_str()).unwrap_or("");
    let target = format!("{base}{path_and_query}");

    let method = reqwest::Method::from_bytes(parts.method.as_str().as_bytes())
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    // Buffer the incoming body — streaming through reqwest is possible
    // but overkill for the small payloads mail/* routes carry.
    let body_bytes = axum::body::to_bytes(body, 32 * 1024 * 1024)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut rb = client.request(method, &target).body(body_bytes.to_vec());
    for (name, value) in parts.headers.iter() {
        // Strip hop-by-hop headers.
        let name_str = name.as_str();
        if matches!(
            name_str,
            "host" | "connection" | "content-length" | "transfer-encoding"
        ) {
            continue;
        }
        rb = rb.header(name_str, value.as_bytes());
    }
    let resp = rb.send().await.map_err(|e| {
        tracing::warn!(error = %e, %target, "proxy to monolith failed");
        StatusCode::BAD_GATEWAY
    })?;

    let status =
        StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let headers_in = resp.headers().clone();
    let body_bytes = resp
        .bytes()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?
        .to_vec();

    let mut builder = Response::builder().status(status);
    for (name, value) in headers_in.iter() {
        let n = name.as_str();
        if matches!(
            n,
            "connection" | "transfer-encoding" | "content-length" | "content-encoding"
        ) {
            continue;
        }
        if let Ok(hn) = axum::http::HeaderName::from_bytes(n.as_bytes()) {
            builder = builder.header(hn, value.as_bytes());
        }
    }
    builder
        .body(Body::from(body_bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
