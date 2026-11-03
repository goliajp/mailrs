//! Team inboxes in the SQL core: the same calls as the kevy core, the facts
//! written to the network kevy by the same code, the reply claim decided
//! under a row lock.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mailrs_core_api::method::shared as s;
use mailrs_core_sidestate::families::email_groups::is_team_member;
use mailrs_core_sidestate::families::shared_inbox::{self, ConvRef, assign};
use mailrs_mailbox::shared_reply::Claim;

use crate::core_rpc::CoreRpcState;

fn conn(state: &CoreRpcState) -> Result<kevy_client::Connection, StatusCode> {
    let url = state
        .net_url
        .as_deref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    kevy_client::Connection::connect(url).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}

fn io_failed(what: &'static str) -> impl Fn(std::io::Error) -> StatusCode {
    move |e| {
        tracing::error!(error = %e, "team inbox: {what} failed");
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

/// The caller must be a current member of the team inbox.
fn member(
    state: &CoreRpcState,
    gid: i64,
    user: &str,
) -> Result<kevy_client::Connection, StatusCode> {
    let mut c = conn(state)?;
    match is_team_member(&mut c, gid, user).map_err(io_failed("membership"))? {
        true => Ok(c),
        false => Err(StatusCode::FORBIDDEN),
    }
}

/// `POST /v1/users/{user}/shared:assign`. The shared writer is synchronous,
/// so it runs off the runtime workers and writes each row through the
/// runtime.
pub async fn assign(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<s::AssignRequest>,
) -> Result<Json<s::AssignResponse>, StatusCode> {
    let rt = tokio::runtime::Handle::current();
    let done = tokio::task::spawn_blocking(move || {
        let mut c = conn(&state)?;
        let r = ConvRef {
            gid: req.gid,
            conv: req.conv,
        };
        let mailbox = state.mailbox.clone();
        let written = assign::assign(&mut c, r, &user, req.to.as_deref(), req.at, |m, t, a| {
            rt.block_on(mailbox.set_assignee(m, t, a))
                .map_err(std::io::Error::other)
        });
        match written {
            Ok((assignee, rows)) => Ok(s::AssignResponse { assignee, rows }),
            Err(assign::Refused::NotMember) => Err(StatusCode::FORBIDDEN),
            Err(assign::Refused::AssigneeNotMember) => Err(StatusCode::BAD_REQUEST),
            Err(assign::Refused::Io(e)) => Err(io_failed("assign")(e)),
        }
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;
    Ok(Json(done))
}

/// `POST /v1/users/{user}/shared:state`
pub async fn state(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<s::StateRequest>,
) -> Result<Json<s::StateResponse>, StatusCode> {
    let mut c = member(&state, req.gid, &user)?;
    let r = ConvRef {
        gid: req.gid,
        conv: req.conv,
    };
    let assignee = assign::current(&mut c, r).map_err(io_failed("assignee"))?;
    let reply = state
        .mailbox
        .reply_state(&r.tag())
        .await
        .map_err(super::failed("reply_state", &user))?;
    Ok(Json(s::StateResponse {
        reply: s::ReplyStateWire {
            seq: reply.seq,
            by: reply.by,
            at: reply.at,
        },
        assignee,
    }))
}

/// `POST /v1/users/{user}/shared:claim-reply`
pub async fn claim(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<s::ClaimReplyRequest>,
) -> Result<Json<s::ClaimReplyResponse>, StatusCode> {
    let mut c = member(&state, req.gid, &user)?;
    let r = ConvRef {
        gid: req.gid,
        conv: req.conv,
    };
    let decided = state
        .mailbox
        .claim_reply(&r.tag(), &user, req.seen_seq, req.force, req.at)
        .await
        .map_err(super::failed("claim_reply", &user))?;
    Ok(Json(match decided {
        Claim::Won { next } => {
            if let Err(e) = shared_inbox::announce_reply(&mut c, r, &next.by, next.seq) {
                tracing::warn!(error = %e, "team inbox: reply not announced");
            }
            s::ClaimReplyResponse::Claimed {
                claim_id: next.claim_id,
                seq: next.seq,
            }
        }
        Claim::Lost { by, at, seq } => s::ClaimReplyResponse::Conflict { by, at, seq },
    }))
}

/// `POST /v1/users/{user}/shared:release-reply`
pub async fn release(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<s::ReleaseReplyRequest>,
) -> Result<Json<s::ReleaseReplyResponse>, StatusCode> {
    member(&state, req.gid, &user)?;
    let r = ConvRef {
        gid: req.gid,
        conv: req.conv,
    };
    let released = state
        .mailbox
        .release_reply(&r.tag(), &req.claim_id)
        .await
        .map_err(super::failed("release_reply", &user))?;
    Ok(Json(s::ReleaseReplyResponse { released }))
}
