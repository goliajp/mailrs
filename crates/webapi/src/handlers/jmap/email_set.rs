//! `Email/set`.
//!
//! Keywords are a message's own; mailboxes belong to its conversation (see
//! `mailbox`), so moving one message moves the conversation it is in, as
//! archiving or deleting it does in the web client. Drafts and Sent are
//! not places a conversation moves to: a message the user wrote is in
//! Sent, or in Drafts while it carries `$draft`.

use mailrs_core_api::method::message_index::{IndexMessage, IndexThread};
use mailrs_jmap::error::JmapMethodError;
use mailrs_mailbox::types::{FLAG_ANSWERED, FLAG_DRAFT, FLAG_FLAGGED, FLAG_SEEN};
use serde_json::{Map, Value, json};

use super::ctx::Ctx;
use super::ids::{self, Blob};
use super::import::{self, NotFiled};
use super::mailbox::{Place, Role};
use super::view::View;
use crate::handlers::compose::Attachment;

/// The flags a client sets through keywords; the rest are kept as they are.
const KEYWORD_FLAGS: u32 = FLAG_SEEN | FLAG_FLAGGED | FLAG_ANSWERED | FLAG_DRAFT;

fn invalid(why: impl Into<String>) -> NotFiled {
    NotFiled("invalidProperties", why.into())
}

fn server(e: impl std::fmt::Display) -> NotFiled {
    NotFiled("serverFail", e.to_string())
}

/// Move a conversation so that it is in `places`. `now` is where it is,
/// `None` for a conversation just filed into the inbox.
pub async fn move_thread(
    ctx: &Ctx,
    tid: &str,
    now: Option<&IndexThread>,
    places: &[Place],
) -> Result<(), String> {
    let core = &ctx.state.core;
    let (trashed, junk, archived, labels) = match now {
        Some(t) => (t.trashed, t.junk, t.archived, t.labels.clone()),
        None => (false, false, false, Vec::new()),
    };
    let has = |r: Role| places.contains(&Place::Role(r));
    let e = |e: mailrs_core_api::error::CoreApiError| e.to_string();
    match (has(Role::Trash), trashed) {
        (true, false) => {
            core.trash_thread(&ctx.user, tid).await.map_err(e)?;
        }
        (false, true) => {
            core.untrash_thread(&ctx.user, tid).await.map_err(e)?;
        }
        _ => {}
    }
    match (has(Role::Junk), junk) {
        (true, false) => {
            core.mark_junk(&ctx.user, tid).await.map_err(e)?;
        }
        (false, true) if has(Role::Inbox) || has(Role::Archive) => {
            core.mark_not_junk(&ctx.user, tid).await.map_err(e)?;
        }
        _ => {}
    }
    match (has(Role::Archive), has(Role::Inbox), archived) {
        (true, false, false) => {
            core.archive_thread(&ctx.user, tid).await.map_err(e)?;
        }
        (false, true, true) => {
            core.unarchive_thread(&ctx.user, tid).await.map_err(e)?;
        }
        _ => {}
    }
    for p in places {
        if let Place::Label(l) = p
            && !labels.contains(l)
        {
            core.set_thread_label(&ctx.user, tid, l, true)
                .await
                .map_err(e)?;
        }
    }
    for l in &labels {
        if !places.contains(&Place::Label(l.clone())) {
            core.set_thread_label(&ctx.user, tid, l, false)
                .await
                .map_err(e)?;
        }
    }
    Ok(())
}

/// The keywords and mailboxes `patch` leaves on `m`: a whole `keywords` /
/// `mailboxIds`, or paths into them.
fn patched(view: &View, m: &IndexMessage, patch: &Value) -> Result<(u32, Vec<Place>), NotFiled> {
    let Some(patch) = patch.as_object() else {
        return Err(invalid("a patch is an object"));
    };
    let mut flags = m.flags;
    let mut places = view.places(m);
    for (path, value) in patch {
        let on = !value.is_null() && value.as_bool() != Some(false);
        if path == "keywords" {
            flags = (flags & !KEYWORD_FLAGS) | import::flags_of(Some(value))?;
        } else if path == "mailboxIds" {
            places = import::mailboxes_of(&json!({ "mailboxIds": value }))?;
        } else if let Some(k) = path.strip_prefix("keywords/") {
            let bit = mailrs_jmap::flags::keyword_to_flag(&k.to_ascii_lowercase());
            if bit == 0 {
                return Err(invalid(format!("keyword {k} is not kept by this server")));
            }
            flags = match on {
                true => flags | bit,
                false => flags & !bit,
            };
        } else if let Some(id) = path.strip_prefix("mailboxIds/") {
            let p = Place::parse(id).ok_or_else(|| invalid(format!("no mailbox {id}")))?;
            places.retain(|q| *q != p);
            if on {
                places.push(p);
            }
        } else {
            return Err(invalid(format!("{path} cannot be changed")));
        }
    }
    if places.is_empty() {
        return Err(invalid("an email is in at least one mailbox"));
    }
    // what the user wrote is in Drafts while it carries `$draft`: moving
    // it out of Drafts (or in) is changing that keyword
    let drafts = places.contains(&Place::Role(Role::Drafts));
    let was = view.places(m).contains(&Place::Role(Role::Drafts));
    if m.own && drafts != was {
        flags = match drafts {
            true => flags | FLAG_DRAFT,
            false => flags & !FLAG_DRAFT,
        };
    }
    Ok((flags, places))
}

async fn update(ctx: &Ctx, view: &View, id: &str, patch: &Value) -> Result<(), NotFiled> {
    let m = ids::parse_email(id)
        .and_then(|uid| view.get(uid))
        .ok_or_else(|| NotFiled("notFound", id.to_string()))?;
    let (flags, places) = patched(view, m, patch)?;
    if flags != m.flags {
        let req = mailrs_core_api::method::admin::SetMessageFlagsRequest { flags };
        ctx.state
            .core
            .set_message_flags(&ctx.user, m.uid, &req)
            .await
            .map_err(server)?;
    }
    let t = &view.index.threads[m.thread as usize];
    move_thread(ctx, view.thread_id(m), Some(t), &places)
        .await
        .map_err(server)
}

/// Destroying a message deletes its conversation, so it is allowed only
/// when every message of the conversation is being destroyed.
async fn destroy(ctx: &Ctx, view: &View, id: &str, all: &[u32]) -> Result<(), NotFiled> {
    let m = ids::parse_email(id)
        .and_then(|uid| view.get(uid))
        .ok_or_else(|| NotFiled("notFound", id.to_string()))?;
    let members = view.thread_members(view.thread_key(m));
    if !members.iter().all(|u| all.contains(u)) {
        return Err(NotFiled(
            "forbidden",
            "messages of a conversation are deleted together; destroy them all or move this one to Trash".into(),
        ));
    }
    ctx.state
        .core
        .delete_thread(&ctx.user, view.thread_id(m))
        .await
        .map_err(server)
}

/// The files an Email object's `attachments` names by `blobId`: an upload
/// of the caller's, or a message or part of one in their mailbox. A blob
/// that is not theirs is not found (`blobNotFound`, RFC 8620 §5.3).
async fn attachments_of(ctx: &Ctx, spec: &Value) -> Result<Vec<Attachment>, NotFiled> {
    let Some(list) = spec.get("attachments").and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };
    let (mut out, mut missing, mut total) = (Vec::new(), Vec::new(), 0usize);
    for a in list {
        let Some(blob_id) = a.get("blobId").and_then(|v| v.as_str()) else {
            return Err(invalid("attachments: each needs a blobId"));
        };
        let Some(bytes) = super::blob::read(ctx, blob_id).await else {
            missing.push(blob_id.to_string());
            continue;
        };
        total += bytes.len();
        if total > super::MAX_SIZE_UPLOAD {
            return Err(NotFiled(
                "tooLarge",
                "attachments: over maxSizeAttachmentsPerEmail".into(),
            ));
        }
        let text = |k: &str| a.get(k).and_then(|v| v.as_str()).map(str::trim);
        let content_type = text("type")
            .filter(|t| t.contains('/') && !t.contains(['\r', '\n', ';']))
            .unwrap_or("application/octet-stream")
            .to_string();
        let filename = text("name")
            .filter(|n| !n.is_empty() && !n.contains(['\r', '\n']))
            .unwrap_or("attachment")
            .to_string();
        out.push(Attachment {
            filename,
            content_type,
            bytes,
        });
    }
    match missing.is_empty() {
        true => Ok(out),
        false => Err(NotFiled("blobNotFound", missing.join(" "))),
    }
}

/// A message built from the Email object (RFC 8621 §4.6): addresses,
/// subject, a text and/or HTML body from `bodyValues`, and attachments by
/// `blobId`.
async fn create(ctx: &Ctx, spec: &Value) -> Result<Value, NotFiled> {
    let places = import::mailboxes_of(spec)?;
    let flags = import::flags_of(spec.get("keywords"))?;
    let files = attachments_of(ctx, spec).await?;
    let bytes = super::submission::compose(spec, files).map_err(invalid)?;
    let uid = import::file(ctx, &bytes, &places, flags).await?;
    Ok(
        json!({ "id": ids::email(uid), "blobId": ids::blob(&Blob::Message(uid)), "size": bytes.len() }),
    )
}

pub async fn set(
    ctx: &Ctx,
    view: &View,
    args: &Value,
    state: &str,
) -> Result<(String, Value), JmapMethodError> {
    ctx.check_account(args)?;
    let creates = args
        .get("create")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let updates = args
        .get("update")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let destroys: Vec<String> = args
        .get("destroy")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if creates.len() + updates.len() + destroys.len() > super::MAX_OBJECTS {
        return Err(super::limit_error("maxObjectsInSet"));
    }
    let (mut created, mut not_created) = (Map::new(), Map::new());
    for (cid, spec) in creates {
        match create(ctx, &spec).await {
            Ok(v) => created.insert(cid, v),
            Err(e) => not_created.insert(cid, e.to_json()),
        };
    }
    let (mut updated, mut not_updated) = (Map::new(), Map::new());
    for (id, patch) in updates {
        match update(ctx, view, &id, &patch).await {
            Ok(()) => updated.insert(id, Value::Null),
            Err(e) => not_updated.insert(id, e.to_json()),
        };
    }
    let all: Vec<u32> = destroys
        .iter()
        .filter_map(|d| ids::parse_email(d))
        .collect();
    let (mut destroyed, mut not_destroyed) = (Vec::new(), Map::new());
    let mut gone_threads = Vec::new();
    for id in destroys {
        let key = ids::parse_email(&id)
            .and_then(|u| view.get(u))
            .map(|m| view.thread_key(m));
        if key.is_some_and(|k| gone_threads.contains(&k)) {
            destroyed.push(id);
            continue;
        }
        match destroy(ctx, view, &id, &all).await {
            Ok(()) => {
                gone_threads.extend(key);
                destroyed.push(id);
            }
            Err(e) => {
                not_destroyed.insert(id, e.to_json());
            }
        }
    }
    Ok((
        "Email/set".into(),
        json!({
            "accountId": ctx.user,
            "oldState": state,
            "newState": state,
            "created": created,
            "notCreated": not_created,
            "updated": updated,
            "notUpdated": not_updated,
            "destroyed": destroyed,
            "notDestroyed": not_destroyed,
        }),
    ))
}

#[cfg(test)]
mod tests {
    use mailrs_core_api::method::message_index::MessageIndex;

    use super::*;

    fn view() -> View {
        View::for_test(MessageIndex {
            threads: vec![IndexThread {
                thread_id: "t".into(),
                archived: false,
                trashed: false,
                junk: false,
                labels: vec!["4".into()],
            }],
            messages: vec![
                IndexMessage {
                    uid: 1,
                    thread: 0,
                    flags: 0,
                    own: false,
                },
                IndexMessage {
                    uid: 2,
                    thread: 0,
                    flags: FLAG_DRAFT,
                    own: true,
                },
            ],
        })
    }

    #[test]
    fn a_patch_sets_keywords_and_mailboxes() {
        let v = view();
        let m = *v.get(1).unwrap();
        let (f, p) = patched(&v, &m, &json!({ "keywords/$seen": true, "mailboxIds/L4": null, "mailboxIds/archive": true, "mailboxIds/inbox": null })).ok().unwrap();
        assert_eq!(f, FLAG_SEEN);
        assert_eq!(p, [Place::Role(Role::Archive)]);
        assert!(patched(&v, &m, &json!({ "keywords/$custom": true })).is_err());
        assert!(patched(&v, &m, &json!({ "mailboxIds": {} })).is_err());
        assert!(patched(&v, &m, &json!({ "subject": "x" })).is_err());
    }

    /// aerc's `onSuccessUpdateEmail` after sending a draft.
    #[test]
    fn a_sent_draft_leaves_drafts_for_sent() {
        let v = view();
        let m = *v.get(2).unwrap();
        let (f, _) = patched(
            &v,
            &m,
            &json!({ "keywords/$draft": null, "mailboxIds/sent": true, "mailboxIds/drafts": null }),
        )
        .ok()
        .unwrap();
        assert_eq!(f & FLAG_DRAFT, 0);
        let (f, _) = patched(
            &v,
            &m,
            &json!({ "mailboxIds/drafts": null, "mailboxIds/sent": true }),
        )
        .ok()
        .unwrap();
        assert_eq!(f & FLAG_DRAFT, 0, "leaving Drafts is sending it");
    }
}
