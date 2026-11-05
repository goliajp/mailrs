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

/// The attachments' names to drop, largest first, for the rest to fit.
fn to_drop(files: &[(&str, u64)], limit: u64) -> Vec<String> {
    let mut by_size: Vec<&(&str, u64)> = files.iter().collect();
    by_size.sort_by_key(|(_, size)| std::cmp::Reverse(*size));
    let mut total: u64 = files.iter().map(|(_, s)| s).sum();
    let mut out = Vec::new();
    for (name, size) in by_size {
        if total <= limit {
            break;
        }
        total -= size;
        out.push(name.to_string());
    }
    out
}

/// Refuse a message whose attachments, wherever they came from (the
/// draft, the form, a forward or a redraft), come to more than
/// `uploads::MAX_ATTACHMENT_BYTES`; the refusal names which to drop.
pub(crate) fn ensure_within_limit(parts: &ComposeParts) -> Result<(), SendError> {
    let files: Vec<(&str, u64)> = parts
        .attachments
        .iter()
        .map(|a| (a.filename.as_str(), a.bytes.len() as u64))
        .collect();
    let total: u64 = files.iter().map(|(_, s)| s).sum();
    let limit = uploads::MAX_ATTACHMENT_BYTES;
    if total <= limit {
        return Ok(());
    }
    Err(SendError::with_body(
        StatusCode::PAYLOAD_TOO_LARGE,
        serde_json::json!({
            "error": format!(
                "the attachments come to more than the {} MiB limit",
                limit / (1024 * 1024)
            ),
            "limit_bytes": limit,
            "total_bytes": total,
            "attachments": files
                .iter()
                .map(|(name, size)| serde_json::json!({ "name": name, "size": size }))
                .collect::<Vec<_>>(),
            "drop": to_drop(&files, limit),
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
    fn an_oversized_send_names_what_to_drop() {
        const MIB: usize = 1024 * 1024;
        assert!(ensure_within_limit(&parts(&[("a", 20 * MIB), ("b", 5 * MIB)])).is_ok());
        let err = ensure_within_limit(&parts(&[
            ("from the draft.pdf", 20 * MIB),
            ("small.txt", MIB),
            ("added.mov", 10 * MIB),
        ]))
        .unwrap_err();
        assert_eq!(err.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            err.body()["drop"],
            serde_json::json!(["from the draft.pdf"])
        );
        assert_eq!(err.body()["limit_bytes"], 25 * MIB);
        assert_eq!(err.body()["attachments"][2]["name"], "added.mov");
        assert_eq!(
            to_drop(&[("a", 10), ("b", 30), ("c", 20)], 35),
            vec!["b".to_string()]
        );
        assert_eq!(
            to_drop(&[("a", 10), ("b", 30), ("c", 20)], 5),
            vec!["b".to_string(), "c".to_string(), "a".to_string()]
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
