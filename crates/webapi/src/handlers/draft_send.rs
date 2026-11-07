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
//!
//! An old client that loaded a draft before files were added to it
//! elsewhere still holds it, and its send would go without them. So a send
//! that names no draft is refused while a draft with files has exactly its
//! recipients and subject: refusing a send that matched by chance costs a
//! resend from a newer client, sending without the files cannot be undone.

use mailrs_core_api::method::admin::DraftWire;
use mailrs_core_sidestate::families::correspondent::all_addresses;
use mailrs_core_sidestate::families::{draft_store, prefs, uploads};
use serde::Deserialize;
use std::collections::BTreeSet;

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

/// Refuse a send that names no draft while a draft with files has its
/// recipients and subject.
pub(crate) async fn refuse_files_left_behind(
    user: &str,
    id: Option<i64>,
    parts: &ComposeParts,
) -> Result<(), SendError> {
    if id.is_some() {
        return Ok(());
    }
    let key = prefs::drafts_key(user);
    let values = with_kevy(move |c| c.hgetall(key.as_bytes()).map_err(std::io::Error::from))?;
    let drafts: Vec<DraftWire> = values
        .iter()
        .skip(1)
        .step_by(2)
        .filter_map(|v| serde_json::from_slice(v).ok())
        .collect();
    if !leaves_files_behind(&drafts, parts) {
        return Ok(());
    }
    Err(SendError::new(
        StatusCode::CONFLICT,
        "this message is a draft with attachments that this app cannot send; update the app or send it from the web",
    ))
}

fn leaves_files_behind(drafts: &[DraftWire], parts: &ComposeParts) -> bool {
    let sent: BTreeSet<String> = parts
        .to
        .iter()
        .chain(&parts.cc)
        .chain(&parts.bcc)
        .flat_map(|list| all_addresses(list))
        .collect();
    if sent.is_empty() {
        return false;
    }
    drafts.iter().any(|d| {
        let kept: BTreeSet<String> = [&d.to, &d.cc, &d.bcc]
            .into_iter()
            .flat_map(|list| all_addresses(list))
            .collect();
        !d.attachments.is_empty() && d.subject.trim() == parts.subject.trim() && kept == sent
    })
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

    fn draft(to: &str, cc: &str, subject: &str, files: usize) -> DraftWire {
        let mut d: DraftWire = serde_json::from_value(serde_json::json!({
            "id": 1, "to": to, "cc": cc, "bcc": "", "subject": subject, "body": "",
            "created_at": 1, "updated_at": 1,
        }))
        .unwrap();
        for i in 0..files {
            d.attachments
                .push(mailrs_core_api::method::admin::DraftAttachment {
                    blob_id: uploads::blob_id(&format!("{i}").repeat(64)),
                    name: format!("f{i}.pdf"),
                    content_type: "application/pdf".into(),
                    size: 3,
                });
        }
        d
    }

    fn send(to: &[&str], cc: &[&str], subject: &str) -> ComposeParts {
        ComposeParts {
            to: to.iter().map(|s| s.to_string()).collect(),
            cc: cc.iter().map(|s| s.to_string()).collect(),
            subject: subject.into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_send_matching_a_draft_with_files_is_refused() {
        let drafts = [draft(
            "Ann <ann@x.example>, bob@y.example",
            "",
            "Invoice",
            1,
        )];
        // same people in another order and case, written another way
        let p = send(&["BOB@y.example", "ann@x.example"], &[], " Invoice ");
        assert!(leaves_files_behind(&drafts, &p));
        // the same people split between to and cc is the same set
        let p = send(&["ann@x.example"], &["bob@y.example"], "Invoice");
        assert!(leaves_files_behind(&drafts, &p));
    }

    #[test]
    fn a_send_differing_from_every_draft_with_files_goes() {
        let drafts = [
            draft("ann@x.example", "", "Invoice", 1),
            draft("bob@y.example", "", "Notes", 0),
        ];
        assert!(!leaves_files_behind(
            &drafts,
            &send(&["ann@x.example"], &[], "Invoice 2")
        ));
        assert!(!leaves_files_behind(
            &drafts,
            &send(&["ann@x.example"], &["bob@y.example"], "Invoice")
        ));
        // a draft without files leaves nothing behind
        assert!(!leaves_files_behind(
            &drafts,
            &send(&["bob@y.example"], &[], "Notes")
        ));
        assert!(!leaves_files_behind(
            &[],
            &send(&["ann@x.example"], &[], "Invoice")
        ));
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
