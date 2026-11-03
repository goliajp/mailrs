//! Sending a reply to a team conversation: the reply is claimed before it
//! is filed or queued, so of two members answering the same message one is
//! told the other already did and sends only if they choose to. Undoing
//! the send gives the claim back.

use std::sync::Arc;

use axum::http::StatusCode;
use mailrs_core_api::method::shared as s;
use mailrs_core_sidestate::families::shared_inbox::{self, ConvRef, HeldClaim, drafts};

use super::now;
use crate::WebState;
use crate::handlers::conversations::map_err;
use crate::handlers::kevy_util::with_kevy;
use crate::handlers::send_error::SendError;

/// What a send names when it answers a team conversation.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub struct SharedReply {
    pub gid: i64,
    pub conv: i64,
    /// The team's reply count when the composer opened.
    pub seen_seq: u64,
    /// Send even though another member answered since.
    #[serde(default)]
    pub force: bool,
}

/// A won claim, until the message is queued.
pub(crate) struct Claimed {
    r: ConvRef,
    claim_id: String,
}

/// Claim the reply. 409 `already_replied` with who answered, and when,
/// when another member answered after the composer opened.
pub(crate) async fn claim(
    state: &Arc<WebState>,
    user: &str,
    reply: &SharedReply,
) -> Result<Claimed, SendError> {
    let req = s::ClaimReplyRequest {
        gid: reply.gid,
        conv: reply.conv,
        seen_seq: reply.seen_seq,
        force: reply.force,
        at: now(),
    };
    let answer = state
        .core
        .shared_claim_reply(user, &req)
        .await
        .map_err(|e| SendError::from(map_err(e)))?;
    match answer {
        s::ClaimReplyResponse::Claimed { claim_id, .. } => Ok(Claimed {
            r: ConvRef {
                gid: reply.gid,
                conv: reply.conv,
            },
            claim_id,
        }),
        s::ClaimReplyResponse::Conflict { by, at, seq } => Err(SendError::with_body(
            StatusCode::CONFLICT,
            serde_json::json!({ "error": "already_replied", "by": by, "at": at, "seq": seq }),
        )),
    }
}

/// The message was queued: the claim stays with it for an undo, the group
/// learns its Message-ID so the answer to it joins the conversation, and
/// the reply being written is no longer being written.
pub(crate) fn sent(user: &str, claimed: Claimed, message_id: &str) {
    let (who, mid) = (user.to_string(), message_id.to_string());
    let done = with_kevy(move |c| {
        let held = HeldClaim {
            gid: claimed.r.gid,
            conv: claimed.r.conv,
            claim_id: claimed.claim_id,
        };
        shared_inbox::hold_claim(c, &who, &mid, &held)?;
        shared_inbox::index_reply(c, claimed.r, &mid)?;
        drafts::clear(c, claimed.r, &who)?;
        Ok(())
    });
    if done.is_err() {
        tracing::warn!(%user, %message_id, "team inbox: sent reply not recorded");
    }
}

/// The claim was won but the message never went out: give it back.
pub(crate) async fn not_sent(state: &Arc<WebState>, user: &str, claimed: Claimed) {
    release(state, user, claimed.r, claimed.claim_id).await;
}

/// The send of `message_id` was undone: release the claim it held, if any.
pub(crate) async fn undone(state: &Arc<WebState>, user: &str, message_id: &str) {
    let (who, mid) = (user.to_string(), message_id.to_string());
    let Ok(Some(held)) = with_kevy(move |c| shared_inbox::take_claim(c, &who, &mid)) else {
        return;
    };
    let r = ConvRef {
        gid: held.gid,
        conv: held.conv,
    };
    release(state, user, r, held.claim_id).await;
}

async fn release(state: &Arc<WebState>, user: &str, r: ConvRef, claim_id: String) {
    let req = s::ReleaseReplyRequest {
        gid: r.gid,
        conv: r.conv,
        claim_id,
    };
    if let Err(e) = state.core.shared_release_reply(user, &req).await {
        tracing::warn!(%user, error = %e, "team inbox: reply claim not released");
    }
}

/// [`claim`] when the send answers a team conversation.
pub(crate) async fn claim_if(
    state: &Arc<WebState>,
    user: &str,
    reply: Option<&SharedReply>,
) -> Result<Option<Claimed>, SendError> {
    match reply {
        Some(r) => Ok(Some(claim(state, user, r).await?)),
        None => Ok(None),
    }
}

/// After the send: [`sent`] when it was queued, [`not_sent`] when not.
pub(crate) async fn settle(
    state: &Arc<WebState>,
    user: &str,
    claimed: Option<Claimed>,
    message_id: &str,
    queued: bool,
) {
    let Some(claimed) = claimed else {
        return;
    };
    match queued {
        true => sent(user, claimed, message_id),
        false => not_sent(state, user, claimed).await,
    }
}
