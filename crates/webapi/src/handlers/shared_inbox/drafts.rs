//! The reply a team member is writing: its author says so as they save it,
//! the rest of the team can read it, and another member can take it over.

use axum::Json;
use axum::extract::{Extension, Path};
use axum::http::StatusCode;
use mailrs_core_api::method::admin::DraftWire;
use mailrs_core_sidestate::families::shared_inbox::ConvRef;
use mailrs_core_sidestate::families::shared_inbox::drafts::{self, DraftPointer, Handoff};

use super::{now, require_member, tell_team};
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

fn changed(r: ConvRef, user: &str) {
    tell_team(
        r,
        user,
        serde_json::json!({ "type": "SharedDraft", "gid": r.gid, "conv": r.conv }),
    );
}

#[derive(Debug, serde::Deserialize)]
pub struct AnnounceBody {
    /// The draft, in the caller's own drafts.
    pub draft_id: i64,
}

/// `PUT /api/shared/{gid}/{conv}/draft` — the caller is writing the reply,
/// saved as their draft `draft_id`. 409 with the holder when another
/// member holds it: after a takeover the earlier author's saves no longer
/// move it.
pub async fn announce(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((gid, conv)): Path<(i64, i64)>,
    Json(body): Json<AnnounceBody>,
) -> Result<Json<DraftPointer>, (StatusCode, Json<Option<DraftPointer>>)> {
    let fail = |s: StatusCode| (s, Json(None));
    require_member(&user, gid).map_err(fail)?;
    let r = ConvRef { gid, conv };
    let who = user.clone();
    let first = with_kevy(move |c| drafts::pointer(c, r))
        .map_err(fail)?
        .is_none();
    let who2 = who.clone();
    match with_kevy(move |c| drafts::announce(c, r, &who2, body.draft_id, now())).map_err(fail)? {
        Ok(p) => {
            if first {
                changed(r, &who);
            }
            Ok(Json(p))
        }
        Err(held) => Err((StatusCode::CONFLICT, Json(Some(held)))),
    }
}

#[derive(Debug, serde::Serialize)]
pub struct Current {
    pub pointer: DraftPointer,
    pub draft: Option<DraftWire>,
}

/// `GET /api/shared/{gid}/{conv}/draft` — the reply being written, to read.
pub async fn current(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((gid, conv)): Path<(i64, i64)>,
) -> Result<Json<Current>, StatusCode> {
    require_member(&user, gid)?;
    let r = ConvRef { gid, conv };
    let found = with_kevy(move |c| {
        let Some(p) = drafts::pointer(c, r)? else {
            return Ok(None);
        };
        let draft = drafts::read(c, &p)?;
        Ok(Some(Current { pointer: p, draft }))
    })?;
    found.map(Json).ok_or(StatusCode::NOT_FOUND)
}

/// `POST /api/shared/{gid}/{conv}/draft/take-over` — the caller continues
/// the reply in a copy of their own.
pub async fn take_over(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((gid, conv)): Path<(i64, i64)>,
) -> Result<Json<Handoff>, StatusCode> {
    require_member(&user, gid)?;
    let r = ConvRef { gid, conv };
    let who = user.clone();
    let h =
        with_kevy(move |c| drafts::take_over(c, r, &who, now()))?.ok_or(StatusCode::CONFLICT)?;
    changed(r, &user);
    Ok(Json(h))
}

/// `DELETE /api/shared/{gid}/{conv}/draft` — the author threw the reply
/// away.
pub async fn clear(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((gid, conv)): Path<(i64, i64)>,
) -> Result<StatusCode, StatusCode> {
    require_member(&user, gid)?;
    let r = ConvRef { gid, conv };
    let who = user.clone();
    if with_kevy(move |c| drafts::clear(c, r, &who))? {
        changed(r, &user);
    }
    Ok(StatusCode::NO_CONTENT)
}
