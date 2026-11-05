//! The one writer of `drafts:{user}`.
//!
//! Every save goes through [`save`], whoever calls it (the web's REST
//! route, either core's route, the MCP tool through a core): it reads the
//! stored draft, merges the request into it and writes it back on one
//! connection. A request without `attachments` keeps the files the draft
//! has; `created_at` is the first save's.

use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

use axum::Json;
use axum::http::StatusCode;
use kevy_client::Connection;
use mailrs_core_api::method::admin::{
    DraftAttachment, DraftWire, SaveDraftRequest, SaveDraftResponse,
};

use super::inline_images;
use super::prefs::{drafts_counter_key, drafts_key};
use super::uploads::{self, MAX_ATTACHMENT_BYTES};

/// Why a save wrote nothing.
#[derive(Debug)]
pub enum SaveError {
    /// Files the request names that are not among the account's uploads,
    /// as `name (blob_id)`.
    Missing(Vec<String>),
    /// The files come to `total` bytes, over [`MAX_ATTACHMENT_BYTES`].
    TooLarge {
        total: u64,
    },
    /// No shared store configured.
    Unavailable,
    Store(io::Error),
}

impl From<io::Error> for SaveError {
    fn from(e: io::Error) -> Self {
        Self::Store(e)
    }
}

impl From<kevy_client::KevyError> for SaveError {
    fn from(e: kevy_client::KevyError) -> Self {
        Self::Store(e.into())
    }
}

impl SaveError {
    /// The status and JSON body a route answers with.
    pub fn into_parts(self) -> (StatusCode, Json<serde_json::Value>) {
        let (status, body) = match self {
            Self::Missing(files) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                serde_json::json!({
                    "error": "these attachments are not among your uploads; upload them again",
                    "missing": files,
                }),
            ),
            Self::TooLarge { total } => (
                StatusCode::PAYLOAD_TOO_LARGE,
                serde_json::json!({
                    "error": format!(
                        "the message is larger than the {} MiB limit",
                        MAX_ATTACHMENT_BYTES / (1024 * 1024)
                    ),
                    "limit_bytes": MAX_ATTACHMENT_BYTES,
                    "total_bytes": total,
                }),
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                serde_json::json!({ "error": "the shared store is not configured" }),
            ),
            Self::Store(e) => {
                tracing::warn!(error = %e, "draft save: store failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    serde_json::json!({ "error": "the draft could not be stored" }),
                )
            }
        };
        (status, Json(body))
    }
}

/// The stored draft `id` of `user`.
pub fn get(conn: &mut Connection, user: &str, id: i64) -> io::Result<Option<DraftWire>> {
    Ok(conn
        .hget(drafts_key(user).as_bytes(), id.to_string().as_bytes())?
        .and_then(|v| serde_json::from_slice(&v).ok()))
}

pub fn put(conn: &mut Connection, user: &str, draft: &DraftWire) -> io::Result<()> {
    let json = serde_json::to_vec(draft).map_err(io::Error::other)?;
    conn.hset(
        drafts_key(user).as_bytes(),
        &[(draft.id.to_string().as_bytes(), json.as_slice())],
    )?;
    Ok(())
}

pub fn remove(conn: &mut Connection, user: &str, id: i64) -> io::Result<()> {
    conn.hdel(drafts_key(user).as_bytes(), &[id.to_string().as_bytes()])?;
    Ok(())
}

fn at(secs: i64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs.max(0) as u64)
}

/// The files `list` names, each checked to be one of `user`'s uploads and
/// marked used at `now`, with the size taken from the stored file.
fn checked(
    root: &Path,
    user: &str,
    list: Vec<DraftAttachment>,
    now: i64,
) -> Result<Vec<DraftAttachment>, SaveError> {
    let mut missing = Vec::new();
    let mut out = Vec::with_capacity(list.len());
    for mut a in list {
        let meta = uploads::path_of(root, user, &a.blob_id)
            .and_then(|p| std::fs::metadata(&p).ok().map(|m| (p, m)));
        match meta {
            Some((path, m)) if m.is_file() => {
                a.size = m.len();
                out.push((path, a));
            }
            _ => missing.push(format!("{} ({})", a.name, a.blob_id)),
        }
    }
    if !missing.is_empty() {
        return Err(SaveError::Missing(missing));
    }
    let total: u64 = out.iter().map(|(_, a)| a.size).sum();
    if total > MAX_ATTACHMENT_BYTES {
        return Err(SaveError::TooLarge { total });
    }
    for (path, _) in &out {
        uploads::touch(path, at(now))?;
    }
    Ok(out.into_iter().map(|(_, a)| a).collect())
}

/// Save `req` as `user`'s draft: in place when it names an id, as a new
/// draft otherwise. Nothing is written when the files it names are wrong.
pub fn save(
    conn: &mut Connection,
    root: &Path,
    user: &str,
    req: SaveDraftRequest,
    now: i64,
) -> Result<SaveDraftResponse, SaveError> {
    let attachments = match req.attachments {
        Some(list) => Some(checked(root, user, list, now)?),
        None => None,
    };
    let old = match req.id {
        Some(id) => get(conn, user, id)?,
        None => None,
    };
    let id = match req.id {
        Some(id) => id,
        None => conn.incr(drafts_counter_key(user).as_bytes())?,
    };
    let (created_at, kept) = match old {
        Some(d) => (d.created_at, d.attachments),
        None => (now, Vec::new()),
    };
    let draft = DraftWire {
        id,
        to: req.to,
        cc: req.cc,
        bcc: req.bcc,
        subject: req.subject,
        body: req.body,
        reply_to_thread_id: req.reply_to_thread_id,
        from: req.from,
        scheduled_at: req.scheduled_at,
        attachments: attachments.unwrap_or(kept),
        created_at,
        updated_at: now,
    };
    put(conn, user, &draft)?;
    for image in inline_images::ids_in(&draft.body) {
        inline_images::keep(conn, image)?;
    }
    Ok(SaveDraftResponse { id })
}

#[cfg(test)]
mod tests;
