//! The ops daemon's two routes on a core.
//!
//! `/v1/ops/status` takes only the daemon's token. A call that is refused
//! is written to the audit log, and so is the daemon's first successful
//! read after it starts; the reads in between are its own polling and are
//! counted by the daemon, not logged here.
//!
//! `/v1/ops/reports:relay` passes fetch's reports on to the daemon as they
//! came. The core cannot write one: the daemon checks fetch's signature.
//!
//! `/v1/ops/alerts` queues the daemon's alert to the configured operator
//! addresses and nowhere else; every call to it, queued or refused, is
//! audited, since it changes state.

use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use super::Handler;
use crate::method::ops::{FIRST_READ_HEADER, OpsAuth};
use crate::method::ops_alert::{OpsAlertQueued, OpsAlertWire, OpsConfig};

pub(super) struct Ops<H> {
    pub handler: Arc<H>,
    pub cfg: OpsConfig,
    pub http: reqwest::Client,
}

/// Why the bearer does not open the route, or `None` when it does.
fn refused(auth: &OpsAuth, headers: &HeaderMap) -> Option<&'static str> {
    use sha2::Digest;
    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    match (auth, bearer) {
        (OpsAuth::Open, _) => None,
        (OpsAuth::Closed, _) => Some("no ops token is configured on this core"),
        (OpsAuth::TokenSha256(_), None) => Some("no bearer"),
        (OpsAuth::TokenSha256(want), Some(token)) => {
            match hex::encode(sha2::Sha256::digest(token.as_bytes())) == *want {
                true => None,
                false => Some("wrong token"),
            }
        }
    }
}

pub(super) async fn status<H: Handler>(
    State(ops): State<Arc<Ops<H>>>,
    headers: HeaderMap,
) -> Response {
    if let Some(why) = refused(&ops.cfg.status, &headers) {
        ops.handler
            .ops_audit("ops.status.refused", why.to_string())
            .await;
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if headers.contains_key(FIRST_READ_HEADER) {
        ops.handler
            .ops_audit(
                "ops.status.first_read",
                "the ops daemon started and read this core".into(),
            )
            .await;
    }
    Json(ops.handler.ops_status().await).into_response()
}

pub(super) async fn relay<H: Handler>(
    State(ops): State<Arc<Ops<H>>>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let head: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return StatusCode::BAD_REQUEST,
    };
    // the relay is fetch's way out of its network, nobody else's
    if head["process"] != "fetch" {
        return StatusCode::FORBIDDEN;
    }
    if head["kind"] == "start" {
        crate::method::ops::witness_fetch_start();
    }
    let Some(to) = &ops.cfg.relay_to else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let signature = headers
        .get(mailrs_ops_report::wire::SIGNATURE_HEADER)
        .cloned()
        .unwrap_or_else(|| axum::http::HeaderValue::from_static(""));
    let sent = ops
        .http
        .post(to)
        .header(mailrs_ops_report::wire::SIGNATURE_HEADER, signature)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .timeout(std::time::Duration::from_secs(5))
        .body(body)
        .send()
        .await;
    match sent {
        Ok(r) => StatusCode::from_u16(r.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
        Err(_) => StatusCode::BAD_GATEWAY,
    }
}

pub(super) async fn alert<H: Handler>(
    State(ops): State<Arc<Ops<H>>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(why) = refused(&ops.cfg.alerts, &headers) {
        ops.handler
            .ops_audit("ops.alert.refused", why.to_string())
            .await;
        return StatusCode::UNAUTHORIZED.into_response();
    }
    // a body naming recipients, or anything else, is not an alert
    let Ok(alert) = serde_json::from_slice::<OpsAlertWire>(&body) else {
        ops.handler
            .ops_audit("ops.alert.refused", "not an alert".into())
            .await;
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(operators) = &ops.cfg.operators else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "no operator addresses configured",
        )
            .into_response();
    };
    let id = alert.id.clone();
    match ops.handler.ops_alert(operators, alert).await {
        Ok(queued) => {
            ops.handler
                .ops_audit(
                    "ops.alert.queued",
                    format!("alert {id} to {} address(es)", queued),
                )
                .await;
            Json(OpsAlertQueued { queued }).into_response()
        }
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, e).into_response(),
    }
}
