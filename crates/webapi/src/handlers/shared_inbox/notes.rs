//! Internal notes on a team conversation: for the team's members only,
//! never part of any message.
//!
//! A note is not mail, and nothing that builds, sends, exports or
//! describes mail reads the keys it lives in: no outbound job, compose
//! quote, export, JMAP / IMAP / POP3 read, webhook payload or model input
//! can carry one. The audit log records who added, changed or deleted
//! which note, by id, never the text. The single exception is the
//! super-administrator view of a deleted note's text, and each read of it
//! is itself recorded before the text is returned.

use axum::Json;
use axum::extract::{Extension, Path};
use axum::http::StatusCode;
use mailrs_core_sidestate::families::shared_inbox::ConvRef;
use mailrs_core_sidestate::families::shared_inbox::notes::{self, DeletedNote, Note, Refused};

use super::{now, require_member, tell_team};
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

/// The longest note kept, in characters.
const MAX_CHARS: usize = 10_000;

fn changed(r: ConvRef, user: &str) {
    tell_team(
        r,
        user,
        serde_json::json!({ "type": "SharedNotes", "gid": r.gid, "conv": r.conv }),
    );
}

fn audit(user: &str, action: &str, r: ConvRef, id: i64) {
    crate::handlers::audit::record(user, action, &r.tag(), &format!("note {id}"));
}

fn refused(e: Refused) -> StatusCode {
    match e {
        Refused::Missing => StatusCode::NOT_FOUND,
        Refused::NotAuthor => StatusCode::FORBIDDEN,
    }
}

fn text_of(body: NoteBody) -> Result<String, StatusCode> {
    let text = body.text.trim().to_string();
    if text.is_empty() || text.chars().count() > MAX_CHARS {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(text)
}

#[derive(Debug, serde::Deserialize)]
pub struct NoteBody {
    pub text: String,
}

/// `GET /api/shared/{gid}/{conv}/notes` — every live note, oldest first,
/// including those written before the reader joined the team.
pub async fn list(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((gid, conv)): Path<(i64, i64)>,
) -> Result<Json<Vec<Note>>, StatusCode> {
    require_member(&user, gid)?;
    let r = ConvRef { gid, conv };
    Ok(Json(with_kevy(move |c| notes::list(c, r))?))
}

#[derive(Debug, serde::Serialize)]
pub struct Added {
    pub id: i64,
}

/// `POST /api/shared/{gid}/{conv}/notes`
pub async fn add(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((gid, conv)): Path<(i64, i64)>,
    Json(body): Json<NoteBody>,
) -> Result<Json<Added>, StatusCode> {
    require_member(&user, gid)?;
    let r = ConvRef { gid, conv };
    let text = text_of(body)?;
    let who = user.to_ascii_lowercase();
    let id = with_kevy(move |c| notes::add(c, r, &who, &text, now()))?;
    audit(&user, "shared_note.add", r, id);
    changed(r, &user);
    Ok(Json(Added { id }))
}

/// `PUT /api/shared/{gid}/{conv}/notes/{id}` — only the author edits.
pub async fn edit(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((gid, conv, id)): Path<(i64, i64, i64)>,
    Json(body): Json<NoteBody>,
) -> Result<StatusCode, StatusCode> {
    require_member(&user, gid)?;
    let r = ConvRef { gid, conv };
    let text = text_of(body)?;
    let who = user.clone();
    with_kevy(move |c| notes::edit(c, r, id, &who, &text, now()))?.map_err(refused)?;
    audit(&user, "shared_note.edit", r, id);
    changed(r, &user);
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /api/shared/{gid}/{conv}/notes/{id}` — only the author deletes;
/// the text is kept out of every view but the audited one below.
pub async fn delete(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((gid, conv, id)): Path<(i64, i64, i64)>,
) -> Result<StatusCode, StatusCode> {
    require_member(&user, gid)?;
    let r = ConvRef { gid, conv };
    let who = user.clone();
    with_kevy(move |c| notes::delete(c, r, id, &who, now()))?.map_err(refused)?;
    audit(&user, "shared_note.delete", r, id);
    changed(r, &user);
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/admin/shared/{gid}/{conv}/deleted-notes/{id}` — what a deleted
/// note said, for a super administrator only. The read is recorded before
/// the text is returned, and a read that cannot be recorded returns
/// nothing.
pub async fn deleted_original(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((gid, conv, id)): Path<(i64, i64, i64)>,
) -> Result<Json<DeletedNote>, StatusCode> {
    crate::handlers::kevy_util::require_super(&state, &user).await?;
    let r = ConvRef { gid, conv };
    let found =
        with_kevy(move |c| notes::deleted_original(c, r, id))?.ok_or(StatusCode::NOT_FOUND)?;
    crate::handlers::audit::record_required(
        &user,
        "shared_note.read_deleted",
        &r.tag(),
        &format!("note {id}"),
    )?;
    Ok(Json(found))
}
