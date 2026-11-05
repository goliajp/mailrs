//! Sending a saved draft, and keeping drafts with files away from clients
//! that cannot send them.
//!
//! A send that names `draft_id` takes the draft's files from the upload
//! store, ahead of the form's own, so the client does not upload them a
//! second time; once the message is queued the draft is deleted, in the
//! same handler. The files stay until the upload sweep finds nothing names
//! them.
//!
//! A client from before drafts held files does not name the draft it sends
//! (it deletes it after the send answers) and sends no version, so the
//! server cannot tell its send from one that meant to leave the files out.
//! What it can do is not show it such a draft: `GET /api/mail/drafts`
//! lists drafts with files only to a client that asks for them with
//! `with_attachments=true`. No client asks yet; the web composer and the
//! apps do when they learn to keep files in drafts.

use mailrs_core_api::method::admin::DraftWire;
use mailrs_core_sidestate::families::{draft_store, uploads};
use serde::Deserialize;

use crate::handlers::compose::{Attachment, ComposeParts};
use crate::handlers::kevy_util::with_kevy;
use crate::handlers::send_error::SendError;
use axum::http::StatusCode;

/// The query of `GET /api/mail/drafts`.
#[derive(Debug, Default, Deserialize)]
pub struct ListDrafts {
    /// The client keeps files in drafts and sends them with `draft_id`.
    #[serde(default)]
    pub with_attachments: bool,
}

impl ListDrafts {
    pub fn shows(&self, d: &DraftWire) -> bool {
        self.with_attachments || d.attachments.is_empty()
    }
}

/// Put the files of draft `id` ahead of the form's in `parts`.
pub(crate) async fn attach(
    user: &str,
    id: Option<i64>,
    parts: &mut ComposeParts,
) -> Result<(), SendError> {
    let Some(id) = id else {
        return Ok(());
    };
    let who = user.to_string();
    let draft = with_kevy(move |c| draft_store::get(c, &who, id))?.ok_or_else(|| {
        SendError::new(
            StatusCode::NOT_FOUND,
            "the draft is gone; it may have been sent already",
        )
    })?;
    let who = user.to_string();
    let read = tokio::task::spawn_blocking(move || {
        let root = uploads::root();
        let mut files = Vec::new();
        let mut missing = Vec::new();
        for a in draft.attachments {
            match uploads::path_of(&root, &who, &a.blob_id).and_then(|p| std::fs::read(p).ok()) {
                Some(bytes) => files.push(Attachment {
                    filename: a.name,
                    content_type: a.content_type,
                    bytes,
                    content_id: None,
                }),
                None => missing.push(a.name),
            }
        }
        (files, missing)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let (files, missing) = read;
    if !missing.is_empty() {
        return Err(SendError::with_body(
            StatusCode::UNPROCESSABLE_ENTITY,
            serde_json::json!({
                "error": "some of the draft's attachments are no longer stored; attach them again",
                "missing": missing,
            }),
        ));
    }
    parts.attachments.splice(0..0, files);
    Ok(())
}

/// What an attachment adds to the message: base64, lines of 76 + CRLF.
fn encoded(bytes: u64) -> u64 {
    bytes.div_ceil(57) * 78
}

/// The attachments' names to drop, largest first, for the message to shrink
/// by `excess` bytes.
fn to_drop(files: &[(&str, u64)], excess: u64) -> Vec<String> {
    let mut by_size: Vec<&(&str, u64)> = files.iter().collect();
    by_size.sort_by_key(|(_, size)| std::cmp::Reverse(*size));
    let mut freed = 0;
    let mut out = Vec::new();
    for (name, size) in by_size {
        if freed >= excess {
            break;
        }
        freed += encoded(*size);
        out.push(name.to_string());
    }
    out
}

/// A send that names a draft: the message, the draft's files counted in
/// it, must fit the whole-message cap every send already meets
/// (`sent::MAX_MESSAGE_BYTES`, what the core files). Refused here rather
/// than by the core so the refusal can name which attachments to drop. A
/// send without a draft is left to that same cap as before.
pub(crate) fn ensure_message_fits(
    draft_id: Option<i64>,
    parts: &ComposeParts,
    message_bytes: usize,
) -> Result<(), SendError> {
    let limit = mailrs_core_api::method::sent::MAX_MESSAGE_BYTES;
    if draft_id.is_none() || message_bytes <= limit {
        return Ok(());
    }
    let files: Vec<(&str, u64)> = parts
        .attachments
        .iter()
        .map(|a| (a.filename.as_str(), a.bytes.len() as u64))
        .collect();
    Err(SendError::with_body(
        StatusCode::PAYLOAD_TOO_LARGE,
        serde_json::json!({
            "error": format!(
                "the message is larger than the {} MiB limit",
                limit / (1024 * 1024)
            ),
            "limit_bytes": limit,
            "message_bytes": message_bytes,
            "attachments": files
                .iter()
                .map(|(name, size)| serde_json::json!({ "name": name, "size": size }))
                .collect::<Vec<_>>(),
            "drop": to_drop(&files, (message_bytes - limit) as u64),
        }),
    ))
}

/// The message is queued: the draft it was is not a draft any more. A
/// failure here is logged, not returned, since the message is on its way
/// and a client told otherwise would send it again.
pub(crate) fn discard(user: &str, id: Option<i64>) {
    let Some(id) = id else {
        return;
    };
    let who = user.to_string();
    if with_kevy(move |c| draft_store::remove(c, &who, id)).is_err() {
        tracing::warn!(%user, id, "send: the sent draft could not be deleted");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(sizes: &[(&str, usize)]) -> ComposeParts {
        ComposeParts {
            attachments: sizes
                .iter()
                .map(|(name, n)| Attachment {
                    filename: name.to_string(),
                    content_type: "application/octet-stream".into(),
                    bytes: vec![0; *n],
                    content_id: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn an_oversized_draft_send_names_what_to_drop() {
        const MIB: usize = 1024 * 1024;
        let limit = mailrs_core_api::method::sent::MAX_MESSAGE_BYTES;
        let p = parts(&[
            ("from the draft.pdf", 24 * MIB),
            ("small.txt", MIB),
            ("added.mov", 20 * MIB),
        ]);
        // without a draft, the core's cap applies as it always did
        assert!(ensure_message_fits(None, &p, limit + 1).is_ok());
        assert!(ensure_message_fits(Some(1), &p, limit).is_ok());
        let err = ensure_message_fits(Some(1), &p, limit + 10 * MIB).unwrap_err();
        assert_eq!(err.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            err.body()["drop"],
            serde_json::json!(["from the draft.pdf"])
        );
        assert_eq!(err.body()["limit_bytes"], limit);
        assert_eq!(err.body()["attachments"][2]["name"], "added.mov");
        assert_eq!(
            to_drop(&[("a", 10), ("b", 30), ("c", 20)], 35),
            vec!["b".to_string()]
        );
        assert_eq!(
            to_drop(&[("a", 570), ("b", 5700), ("c", 2850)], 8000),
            vec!["b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn a_client_that_does_not_ask_sees_no_draft_with_files() {
        let mut d: DraftWire = serde_json::from_value(serde_json::json!({
            "id": 1, "to": "", "cc": "", "bcc": "", "subject": "", "body": "",
            "created_at": 1, "updated_at": 1,
        }))
        .unwrap();
        let old = ListDrafts::default();
        let new = ListDrafts {
            with_attachments: true,
        };
        assert!(old.shows(&d) && new.shows(&d));
        d.attachments
            .push(mailrs_core_api::method::admin::DraftAttachment {
                blob_id: uploads::blob_id(&"a".repeat(64)),
                name: "f.pdf".into(),
                content_type: "application/pdf".into(),
                size: 3,
            });
        assert!(!old.shows(&d));
        assert!(new.shows(&d));
    }
}
