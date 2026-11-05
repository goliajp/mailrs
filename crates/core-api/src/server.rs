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

use crate::method::ops::{OpsAuth, OpsStatusWire};
use crate::types::HealthResponse;

mod ops;
mod thread_events;
pub use thread_events::{ThreadEventHit, ThreadEventSink, thread_events};

/// Trait the backend implements to host a `mailrs-core-api` server.
///
/// `core` (PG-backed) and `fastcore` (KV-backed) each impl this with
/// their own concrete store inside.
///
/// Edition 2024 native async-in-trait — no `async-trait` dep needed.
pub trait Handler: Send + Sync + 'static {
    /// Reports backend kind + readiness for the /healthz probe.
    fn healthz(&self) -> impl std::future::Future<Output = HealthResponse> + Send;

    /// Deeper readiness probe — verifies the backend pool is open.
    fn readyz(&self) -> impl std::future::Future<Output = HealthResponse> + Send;

    /// What the ops daemon reads: drain progress and quarantine records.
    fn ops_status(&self) -> impl std::future::Future<Output = OpsStatusWire> + Send;

    /// Write one line to the audit log with the ops daemon as the actor: a
    /// refused call to its route, or its first read after it started.
    fn ops_audit(
        &self,
        action: &'static str,
        detail: String,
    ) -> impl std::future::Future<Output = ()> + Send;
}

/// Build the base router: healthz/readyz (no auth), the ops daemon's
/// status route (its own token) and fetch's report relay (see
/// [`crate::method::ops`]). The relay goes to `MAILRS_OPS_URL`.
pub fn base_router<H: Handler>(handler: Arc<H>) -> Router {
    let relay_to = std::env::var("MAILRS_OPS_URL")
        .ok()
        .filter(|u| !u.trim().is_empty())
        .map(|u| {
            format!(
                "{}{}",
                u.trim_end_matches('/'),
                mailrs_ops_report::wire::REPORTS_PATH
            )
        });
    base_router_with_ops(handler, OpsAuth::from_env(), relay_to)
}

pub fn base_router_with_ops<H: Handler>(
    handler: Arc<H>,
    auth: OpsAuth,
    relay_to: Option<String>,
) -> Router {
    let ops_state = Arc::new(ops::Ops {
        handler: handler.clone(),
        auth,
        relay_to,
        http: reqwest::Client::new(),
    });
    let ops_route = Router::new()
        .route(crate::method::ops::PATH_OPS_STATUS, get(ops::status::<H>))
        .route(
            crate::method::ops::PATH_OPS_RELAY,
            axum::routing::post(ops::relay::<H>).layer(axum::extract::DefaultBodyLimit::max(
                mailrs_ops_report::wire::MAX_REPORT_BYTES,
            )),
        )
        .with_state(ops_state);
    Router::new()
        .route(
            crate::method::health::PATH_HEALTHZ,
            get(healthz_handler::<H>),
        )
        .route(crate::method::health::PATH_READYZ, get(readyz_handler::<H>))
        .with_state(handler)
        .merge(ops_route)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::method::ops::DrainProgressWire;
    use crate::types::BackendKind;
    use sha2::Digest;
    use tower::ServiceExt;

    #[derive(Default)]
    struct Core(std::sync::Mutex<Vec<&'static str>>);

    fn health() -> HealthResponse {
        HealthResponse {
            version: "v1".into(),
            backend: BackendKind::Kevy,
            ready: true,
            memory: None,
            spool: None,
            set_aside: None,
            parsing: None,
        }
    }

    impl Handler for Core {
        async fn healthz(&self) -> HealthResponse {
            health()
        }
        async fn readyz(&self) -> HealthResponse {
            health()
        }
        async fn ops_status(&self) -> OpsStatusWire {
            OpsStatusWire {
                build: mailrs_ops_report::build::build(),
                fetch_starts_witnessed: 0,
                drain: DrainProgressWire {
                    ticks: 0,
                    delivered: 0,
                    last_tick_at: None,
                },
                quarantined_total: 0,
                quarantined: vec![],
            }
        }
        async fn ops_audit(&self, action: &'static str, _detail: String) {
            self.0.lock().unwrap().push(action);
        }
    }

    async fn status(core: &Arc<Core>, auth: OpsAuth, bearer: Option<&str>, first: bool) -> u16 {
        let mut req = axum::http::Request::get(crate::method::ops::PATH_OPS_STATUS);
        if let Some(b) = bearer {
            req = req.header("authorization", format!("Bearer {b}"));
        }
        if first {
            req = req.header(crate::method::ops::FIRST_READ_HEADER, "1");
        }
        base_router_with_ops(core.clone(), auth, None)
            .oneshot(req.body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
            .as_u16()
    }

    /// The daemon's token opens the route; the core's own secret, another
    /// token or none do not, and a deployment without the hash is closed.
    /// Each refusal is audited, and of the successful reads only the first
    /// after the daemon started.
    #[tokio::test]
    async fn the_ops_route_takes_only_the_daemon_token() {
        let core = Arc::new(Core::default());
        let hash = hex::encode(sha2::Sha256::digest(b"daemon-token"));
        let by_token = || OpsAuth::TokenSha256(hash.clone());
        assert_eq!(
            status(&core, by_token(), Some("daemon-token"), true).await,
            200
        );
        assert_eq!(
            status(&core, by_token(), Some("daemon-token"), false).await,
            200
        );
        assert_eq!(
            status(&core, by_token(), Some("core-secret"), false).await,
            401
        );
        assert_eq!(status(&core, by_token(), None, false).await, 401);
        assert_eq!(
            status(&core, OpsAuth::Closed, Some("daemon-token"), false).await,
            401
        );
        assert_eq!(status(&core, OpsAuth::Open, None, false).await, 200);
        assert_eq!(
            *core.0.lock().unwrap(),
            [
                "ops.status.first_read",
                "ops.status.refused",
                "ops.status.refused",
                "ops.status.refused"
            ]
        );
    }

    async fn relay(body: serde_json::Value) -> u16 {
        let req = axum::http::Request::post(crate::method::ops::PATH_OPS_RELAY)
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();
        base_router_with_ops(Arc::new(Core::default()), OpsAuth::Open, None)
            .oneshot(req)
            .await
            .unwrap()
            .status()
            .as_u16()
    }

    /// Only fetch's reports are relayed, and each start is counted as it
    /// passes, whether or not the daemon can take it.
    #[tokio::test]
    async fn the_relay_carries_fetch_reports_only_and_counts_its_starts() {
        let before = crate::method::ops::fetch_starts_witnessed();
        let start = serde_json::json!({"process": "fetch", "kind": "start"});
        assert_eq!(relay(start).await, 503, "no daemon configured");
        assert_eq!(crate::method::ops::fetch_starts_witnessed(), before + 1);
        let other = serde_json::json!({"process": "receiver", "kind": "start"});
        assert_eq!(relay(other).await, 403);
        assert_eq!(crate::method::ops::fetch_starts_witnessed(), before + 1);
    }
}
