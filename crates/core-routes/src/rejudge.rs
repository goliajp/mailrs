//! Re-judging after a reputation change, as the operator sees it: what it
//! has done since the process started, a round now, and the batches it
//! took back, put back whole or decided on. The re-judge itself is
//! `mailrs_intake::rejudge`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::admin::{
    ConfirmRejudgeBatchResponse, DecideRejudgeBatchRequest, DiscardRejudgeBatchResponse,
    RejudgeBatchWire, RejudgeBatchesResponse, ReleaseRejudgeBatchRequest,
    ReleaseRejudgeBatchResponse,
};

use mailrs_intake::rejudge::recall::{self, Refused};
use mailrs_intake::rejudge::{COUNTS, RejudgeStore, judge, tick};

/// A core's state as the re-judge routes see it: the store the re-judge
/// runs over, for one request.
pub trait RejudgeHost: Send + Sync + 'static {
    type Store: RejudgeStore;

    fn rejudge_store(self: &Arc<Self>) -> Arc<Self::Store>;
}

/// `GET /v1/admin/rejudge-batches`
pub async fn batches<H: RejudgeHost>(State(host): State<Arc<H>>) -> Response {
    let store = host.rejudge_store();
    let found = tokio::task::spawn_blocking(move || recall::batches(&*store)).await;
    match found {
        Ok(items) => Json(RejudgeBatchesResponse {
            items: items
                .into_iter()
                .map(|b| RejudgeBatchWire {
                    observation: b.observation,
                    at: b.at,
                    conversations: b.conversations,
                    released_at: b.released_at,
                    pending: b.pending,
                    would_hold: b.would_hold,
                    rules: b.rules,
                    discarded_at: b.discarded_at,
                    confirmed_at: b.confirmed_at,
                })
                .collect(),
        })
        .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "rejudge batches: listing panicked");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Run a decision on one batch off the runtime and answer with `ok` of its
/// count, 404 for a missing batch, 409 for one in the wrong state.
async fn decided<T: serde::Serialize>(
    what: &'static str,
    run: impl FnOnce() -> Result<u64, Refused> + Send + 'static,
    ok: impl FnOnce(u64) -> T,
) -> Response {
    match tokio::task::spawn_blocking(run).await {
        Ok(Ok(n)) => Json(ok(n)).into_response(),
        Ok(Err(Refused::Missing)) => StatusCode::NOT_FOUND.into_response(),
        Ok(Err(Refused::Conflict)) => StatusCode::CONFLICT.into_response(),
        Err(e) => {
            tracing::error!(error = %e, what, "rejudge batch decision panicked");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /v1/admin/rejudge-batches/{observation}/release`
pub async fn release<H: RejudgeHost>(
    State(host): State<Arc<H>>,
    Path(observation): Path<String>,
    Json(req): Json<ReleaseRejudgeBatchRequest>,
) -> Response {
    let store = host.rejudge_store();
    decided(
        "release",
        move || recall::release_batch(&*store, &observation, &req.actor),
        |released| ReleaseRejudgeBatchResponse { released },
    )
    .await
}

/// `POST /v1/admin/rejudge-batches/{observation}/confirm`
pub async fn confirm<H: RejudgeHost>(
    State(host): State<Arc<H>>,
    Path(observation): Path<String>,
    Json(req): Json<DecideRejudgeBatchRequest>,
) -> Response {
    let store = host.rejudge_store();
    decided(
        "confirm",
        move || recall::confirm_batch(&*store, &observation, &req.actor),
        |held| ConfirmRejudgeBatchResponse { held },
    )
    .await
}

/// `POST /v1/admin/rejudge-batches/{observation}/discard`
pub async fn discard<H: RejudgeHost>(
    State(host): State<Arc<H>>,
    Path(observation): Path<String>,
    Json(req): Json<DecideRejudgeBatchRequest>,
) -> Response {
    let store = host.rejudge_store();
    decided(
        "discard",
        move || recall::discard_batch(&*store, &observation, &req.actor),
        |would_hold| DiscardRejudgeBatchResponse { would_hold },
    )
    .await
}

/// `GET /v1/admin/maintenance:rejudge-status`: what the re-judge has done
/// since the process started, and what it would have done. Read-only.
pub async fn status() -> Json<serde_json::Value> {
    Json(COUNTS.snapshot())
}

/// `POST /v1/admin/maintenance:rejudge-now`: one round now rather than at
/// the next announcement or poll, for an operator timing the path on a
/// copy. It acts only when acting is switched on, like every round.
pub async fn now<H: RejudgeHost>(State(host): State<Arc<H>>) -> Response {
    let store = host.rejudge_store();
    let round = tokio::task::spawn_blocking(move || {
        let mut judge = judge::Judge::new(&*store);
        tick(&*store, &mut judge)
    })
    .await;
    match round {
        Ok(Ok(judged)) => Json(serde_json::json!({
            "judged": judged,
            "since_start": COUNTS.snapshot(),
        }))
        .into_response(),
        Ok(Err(e)) => (StatusCode::SERVICE_UNAVAILABLE, e.to_string()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
