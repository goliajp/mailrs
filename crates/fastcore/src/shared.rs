//! Team inboxes in this core: the team columns read back onto list rows,
//! "assigned to me", and the core-api calls that assign a conversation and
//! claim its reply. The facts are in the network kevy, written by the same
//! code the SQL core runs; the claim is decided in this store's atomic
//! block.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mailrs_core_api::method::shared as s;
use mailrs_core_api::types::{ConversationFilter, ConversationSummaryWire};
use mailrs_core_sidestate::families::email_groups::is_team_member;
use mailrs_core_sidestate::families::shared_inbox::{self, ConvRef, assign};
use mailrs_mailbox_kevy::ThreadRow;
use mailrs_mailbox_kevy::shared_reply::Claim;

use crate::FastcoreState;

/// Each row's team conversation and assignee, off the reader's own row.
pub(crate) fn with_shared(
    state: &FastcoreState,
    user: &str,
    mut items: Vec<ConversationSummaryWire>,
) -> Vec<ConversationSummaryWire> {
    for item in &mut items {
        let (tag, assignee) = state
            .mailbox
            .shared_fields(user, &item.thread_id)
            .unwrap_or_default();
        item.shared_conv = tag;
        item.assignee = assignee;
    }
    items
}

/// The reader's live team conversations assigned to the reader.
pub(crate) fn assigned_page(
    state: &FastcoreState,
    user: &str,
    f: &ConversationFilter,
    limit: usize,
) -> std::io::Result<Vec<ThreadRow>> {
    let tids = state.mailbox.list_thread_ids_assigned_via_table(
        user,
        &user.to_ascii_lowercase(),
        f.before_ts.map(|ts| ts - 1),
        limit,
    )?;
    let mut rows = Vec::with_capacity(tids.len());
    for tid in tids {
        if let Some(row) = state.mailbox.get_thread_for_user(user, &tid)? {
            rows.push(row);
        }
    }
    Ok(rows)
}

fn failed(what: &'static str) -> impl Fn(std::io::Error) -> StatusCode {
    move |e| {
        tracing::error!(error = %e, "team inbox: {what} failed");
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

/// The caller must be a current member of the team inbox.
fn member(
    state: &FastcoreState,
    gid: i64,
    user: &str,
) -> Result<kevy_client::Connection, StatusCode> {
    let mut conn = state.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    match is_team_member(&mut conn, gid, user).map_err(failed("membership"))? {
        true => Ok(conn),
        false => Err(StatusCode::FORBIDDEN),
    }
}

/// `POST /v1/users/{user}/shared:assign`
pub(crate) async fn assign_route(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<s::AssignRequest>,
) -> Result<Json<s::AssignResponse>, StatusCode> {
    let mut conn = state.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let r = ConvRef {
        gid: req.gid,
        conv: req.conv,
    };
    let mailbox = &state.mailbox;
    let done = assign::assign(&mut conn, r, &user, req.to.as_deref(), req.at, |m, t, a| {
        mailbox.set_assignee(m, t, a)
    });
    match done {
        Ok((assignee, rows)) => Ok(Json(s::AssignResponse { assignee, rows })),
        Err(assign::Refused::NotMember) => Err(StatusCode::FORBIDDEN),
        Err(assign::Refused::AssigneeNotMember) => Err(StatusCode::BAD_REQUEST),
        Err(assign::Refused::Io(e)) => Err(failed("assign")(e)),
    }
}

/// `POST /v1/users/{user}/shared:state`
pub(crate) async fn state_route(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<s::StateRequest>,
) -> Result<Json<s::StateResponse>, StatusCode> {
    let mut conn = member(&state, req.gid, &user)?;
    let r = ConvRef {
        gid: req.gid,
        conv: req.conv,
    };
    let reply = state
        .mailbox
        .reply_state(&r.tag())
        .map_err(failed("reply state"))?;
    let assignee = assign::current(&mut conn, r).map_err(failed("assignee"))?;
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
pub(crate) async fn claim_route(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<s::ClaimReplyRequest>,
) -> Result<Json<s::ClaimReplyResponse>, StatusCode> {
    let mut conn = member(&state, req.gid, &user)?;
    let r = ConvRef {
        gid: req.gid,
        conv: req.conv,
    };
    let decided = state
        .mailbox
        .claim_reply(&r.tag(), &user, req.seen_seq, req.force, req.at)
        .map_err(failed("claim"))?;
    Ok(Json(claimed(&mut conn, r, decided)))
}

/// The answer to a claim, and the team told when it won.
pub(crate) fn claimed(
    conn: &mut kevy_client::Connection,
    r: ConvRef,
    decided: Claim,
) -> s::ClaimReplyResponse {
    match decided {
        Claim::Won { next } => {
            if let Err(e) = shared_inbox::announce_reply(conn, r, &next.by, next.seq) {
                tracing::warn!(error = %e, "team inbox: reply not announced");
            }
            s::ClaimReplyResponse::Claimed {
                claim_id: next.claim_id,
                seq: next.seq,
            }
        }
        Claim::Lost { by, at, seq } => s::ClaimReplyResponse::Conflict { by, at, seq },
    }
}

/// `POST /v1/users/{user}/shared:release-reply`
pub(crate) async fn release_route(
    State(state): State<Arc<FastcoreState>>,
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
        .map_err(failed("release"))?;
    Ok(Json(s::ReleaseReplyResponse { released }))
}
