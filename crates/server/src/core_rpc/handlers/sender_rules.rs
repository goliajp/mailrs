//! `senders:reclassify`, `senders:restore`, `senders:retain`: a reader's
//! rules for one sender applied to this store (`mailrs_intake::sender_ops`).

use std::io;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mailrs_core_api::method::senders as wire;
use mailrs_core_sidestate::families::sender_rules::{Disposal, Moved, SenderRule};
use mailrs_intake::sender_ops;

use crate::core_rpc::CoreRpcState;
use crate::intake::{PgCore, PgDirectory};

fn core(state: &CoreRpcState) -> PgCore {
    PgCore {
        dir: PgDirectory::new(
            state.domain.clone(),
            state.maildir_root.clone(),
            state.net_url.clone(),
        ),
        mailbox: state.mailbox.clone(),
        event_bus: state.event_bus.clone(),
    }
}

/// Run `f` against this store on a blocking thread (the store's calls block
/// on the runtime); a bad bucket or disposal is the caller's mistake.
async fn blocking<T: Send + 'static>(
    state: &CoreRpcState,
    f: impl FnOnce(&PgCore) -> io::Result<T> + Send + 'static,
) -> Result<T, StatusCode> {
    // built here, on a runtime thread: the directory keeps its handle
    let core = core(state);
    match tokio::task::spawn_blocking(move || f(&core)).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) if e.kind() == io::ErrorKind::InvalidInput => Err(StatusCode::BAD_REQUEST),
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "sender rule action failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
        Err(e) => {
            tracing::warn!(error = %e, "sender rule action panicked");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

fn is_bucket(name: &str) -> bool {
    sender_ops::BUCKETS.contains(&name)
}

pub async fn reclassify(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<wire::ReclassifySenderRequest>,
) -> Result<Json<wire::ReclassifySenderResponse>, StatusCode> {
    if !is_bucket(&req.bucket) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let moved = blocking(&state, move |c| {
        sender_ops::reclassify(c, &user, &req.sender, &req.bucket)
    })
    .await?;
    Ok(Json(wire::ReclassifySenderResponse {
        moved: moved
            .into_iter()
            .map(|m| wire::MovedWire {
                thread_id: m.thread_id,
                from_bucket: m.from_bucket,
            })
            .collect(),
    }))
}

pub async fn restore(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<wire::RestoreSenderRequest>,
) -> Result<Json<wire::RestoreSenderResponse>, StatusCode> {
    let moved: Vec<Moved> = req
        .moved
        .into_iter()
        .map(|m| Moved {
            thread_id: m.thread_id,
            from_bucket: m.from_bucket,
        })
        .collect();
    let restored = blocking(&state, move |c| {
        sender_ops::restore(c, &user, &req.sender, &req.to_bucket, &moved)
    })
    .await?;
    Ok(Json(wire::RestoreSenderResponse { restored }))
}

pub async fn retain(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<wire::RetainSenderRequest>,
) -> Result<Json<wire::RetainSenderResponse>, StatusCode> {
    let disposal = match req.disposal.as_str() {
        "archive" => Disposal::Archive,
        "trash" => Disposal::Trash,
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    let rule = SenderRule {
        keep_latest: req.keep_latest,
        older_than_days: req.older_than_days,
        disposal,
        ..Default::default()
    };
    let disposed = blocking(&state, move |c| {
        sender_ops::retain(c, &user, &req.sender, &rule, req.now)
    })
    .await?;
    Ok(Json(wire::RetainSenderResponse { disposed }))
}

/// The age rules, hourly; six hours apart while nothing is due, which costs
/// one read of the set of readers with such a rule.
pub fn spawn_retention(state: Arc<CoreRpcState>) {
    use std::time::Duration;
    const BASE: Duration = Duration::from_secs(3600);
    const MAX: Duration = Duration::from_secs(6 * 3600);
    tokio::spawn(async move {
        let mut wait = BASE;
        loop {
            tokio::time::sleep(wait).await;
            let Some(url) = state.net_url.clone() else {
                return;
            };
            let gone = blocking(&state, move |c| {
                let mut conn = kevy_client::Connection::connect(&url)?;
                sender_ops::sweep(c, &mut conn, chrono::Utc::now().timestamp())
            })
            .await
            .unwrap_or(0);
            wait = match gone {
                0 => (wait * 2).min(MAX),
                n => {
                    tracing::info!(disposed = n, "sender retention: conversations let go");
                    BASE
                }
            };
        }
    });
}
