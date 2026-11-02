//! Team inboxes for the browser: one team conversation's state, assigning
//! it, and who has it open. Notes, the reply being written and the reply
//! claim are in their own modules.
//!
//! Everything here is for current members of the shared group only, read
//! against the member set on every call: an administrator who is not a
//! member is refused like anyone else, and a member taken out loses it at
//! once while keeping their own copies of the mail.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use mailrs_core_api::method::shared as s;
use mailrs_core_sidestate::families::email_groups;
use mailrs_core_sidestate::families::shared_inbox::{
    self, ConvRef, assign, drafts as team_drafts, presence,
};

use crate::WebState;
use crate::handlers::conversations::{AuthedUser, map_err};
use crate::handlers::kevy_util::with_kevy;

pub mod claim;
pub mod drafts;
pub mod notes;

pub(crate) fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The caller must be a current member of team inbox `gid`.
pub(crate) fn require_member(user: &str, gid: i64) -> Result<(), StatusCode> {
    let who = user.to_string();
    match with_kevy(move |c| email_groups::is_team_member(c, gid, &who))? {
        true => Ok(()),
        false => Err(StatusCode::FORBIDDEN),
    }
}

/// Tell the other members' open pages that something in the conversation
/// changed. The event names what changed and nothing more: the page asks
/// for the rest through these routes, as a member.
pub(crate) fn tell_team(r: ConvRef, except: &str, payload: serde_json::Value) {
    let except = except.to_ascii_lowercase();
    let sent = with_kevy(move |c| {
        let others: Vec<String> = shared_inbox::members(c, r.gid)?
            .into_iter()
            .filter(|m| *m != except)
            .collect();
        shared_inbox::publish(c, &others, &payload)
    });
    if sent.is_err() {
        tracing::warn!(gid = r.gid, conv = r.conv, "team inbox: change not pushed");
    }
}

/// Everything a member's page shows about a team conversation besides the
/// mail and the notes.
#[derive(Debug, serde::Serialize)]
pub struct Overview {
    pub gid: i64,
    pub conv: i64,
    pub members: Vec<String>,
    pub assignee: Option<String>,
    pub assignments: Vec<assign::AssignFact>,
    /// The reply count a composer remembers as `seen_seq`, and who sent
    /// the last one.
    pub reply: s::ReplyStateWire,
    pub present: Vec<Presence>,
    /// The reply being written, if anyone is.
    pub draft: Option<team_drafts::DraftPointer>,
    pub handoffs: Vec<team_drafts::Handoff>,
}

#[derive(Debug, serde::Serialize)]
pub struct Presence {
    pub user: String,
    pub state: presence::State,
}

/// `GET /api/shared/{gid}/{conv}`
pub async fn overview(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((gid, conv)): Path<(i64, i64)>,
) -> Result<Json<Overview>, StatusCode> {
    require_member(&user, gid)?;
    let r = ConvRef { gid, conv };
    let core = state
        .core
        .shared_state(&user, &s::StateRequest { gid, conv })
        .await
        .map_err(map_err)?;
    let (members, assignments, present, draft, handoffs) = with_kevy(move |c| {
        let members = shared_inbox::members(c, gid)?;
        let present = presence::present(c, r, &members)?;
        Ok((
            members,
            assign::history(c, r)?,
            present,
            team_drafts::pointer(c, r)?,
            team_drafts::handoffs(c, r)?,
        ))
    })?;
    Ok(Json(Overview {
        gid,
        conv,
        members,
        assignee: core.assignee,
        assignments,
        reply: core.reply,
        present: present
            .into_iter()
            .map(|(user, state)| Presence { user, state })
            .collect(),
        draft,
        handoffs,
    }))
}

#[derive(Debug, serde::Deserialize)]
pub struct AssignBody {
    /// The member to assign it to; absent or null takes it back.
    #[serde(default)]
    pub to: Option<String>,
}

/// `POST /api/shared/{gid}/{conv}/assign`
pub async fn assign_conv(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((gid, conv)): Path<(i64, i64)>,
    Json(body): Json<AssignBody>,
) -> Result<Json<s::AssignResponse>, StatusCode> {
    let req = s::AssignRequest {
        gid,
        conv,
        to: body.to,
        at: now(),
    };
    let done = state
        .core
        .shared_assign(&user, &req)
        .await
        .map_err(map_err)?;
    Ok(Json(done))
}

#[derive(Debug, serde::Deserialize)]
pub struct PresenceBody {
    pub state: presence::State,
}

/// `POST /api/shared/{gid}/{conv}/presence` — the page's heartbeat, every
/// 20 seconds while the conversation is open.
pub async fn heartbeat(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((gid, conv)): Path<(i64, i64)>,
    Json(body): Json<PresenceBody>,
) -> Result<StatusCode, StatusCode> {
    require_member(&user, gid)?;
    let r = ConvRef { gid, conv };
    let who = user.clone();
    let changed = with_kevy(move |c| presence::heartbeat(c, r, &who, body.state))?;
    if changed {
        tell_team(
            r,
            &user,
            serde_json::json!({ "type": "SharedPresence", "gid": gid, "conv": conv,
                                "member": user.to_ascii_lowercase(), "state": body.state }),
        );
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /api/shared/{gid}/{conv}/presence` — the page closed the
/// conversation.
pub async fn leave(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((gid, conv)): Path<(i64, i64)>,
) -> Result<StatusCode, StatusCode> {
    require_member(&user, gid)?;
    let r = ConvRef { gid, conv };
    let who = user.clone();
    if with_kevy(move |c| presence::leave(c, r, &who))? {
        tell_team(
            r,
            &user,
            serde_json::json!({ "type": "SharedPresence", "gid": gid, "conv": conv,
                                "member": user.to_ascii_lowercase(), "state": null }),
        );
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize)]
pub struct SharedFlag {
    pub shared: bool,
}

/// `PUT /api/admin/email-groups/{id}/shared` — make a group a team inbox,
/// or a plain distribution list again.
pub async fn set_group_shared(
    _: crate::session::confirm::Confirmed,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(id): Path<i64>,
    Json(body): Json<SharedFlag>,
) -> Result<StatusCode, StatusCode> {
    let on = body.shared;
    if !with_kevy(move |c| email_groups::set_shared(c, id, on))? {
        return Err(StatusCode::NOT_FOUND);
    }
    crate::handlers::audit::record(
        &actor,
        "email_group.shared",
        &id.to_string(),
        &on.to_string(),
    );
    Ok(StatusCode::NO_CONTENT)
}
