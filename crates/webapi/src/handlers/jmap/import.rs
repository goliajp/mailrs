//! `Email/import`, and the filing `Email/set` create shares with it.
//!
//! What the user files into Drafts or Sent is theirs: it is filed as sent
//! mail (a draft keeps `$draft`, which puts it in Drafts until it goes).
//! Anything else is filed as imported mail and then moved where
//! `mailboxIds` says, the way a client moving it would.

use mailrs_jmap::error::JmapMethodError;
use mailrs_mailbox::types::{FLAG_DRAFT, FLAG_FLAGGED, FLAG_SEEN};
use serde_json::{Map, Value, json};

use super::blob;
use super::ctx::Ctx;
use super::email_set;
use super::ids::{self, Blob};
use super::mailbox::{Place, Role};

/// Why a message could not be filed, as a SetError.
pub struct NotFiled(pub &'static str, pub String);

impl NotFiled {
    pub fn to_json(&self) -> Value {
        json!({ "type": self.0, "description": self.1 })
    }
}

fn invalid(why: impl Into<String>) -> NotFiled {
    NotFiled("invalidProperties", why.into())
}

/// A full mailbox is `overQuota` (RFC 8620 §5.3), not a server failure.
fn not_filed(e: mailrs_core_api::error::CoreApiError) -> NotFiled {
    match e.status_code() {
        507 => NotFiled("overQuota", e.to_string()),
        _ => NotFiled("serverFail", e.to_string()),
    }
}

/// The mailboxes `spec` names; at least one.
pub fn mailboxes_of(spec: &Value) -> Result<Vec<Place>, NotFiled> {
    let Some(map) = spec.get("mailboxIds").and_then(|v| v.as_object()) else {
        return Err(invalid("mailboxIds is required"));
    };
    let mut out = Vec::new();
    for (id, on) in map {
        if on.as_bool() != Some(true) {
            continue;
        }
        out.push(Place::parse(id).ok_or_else(|| invalid(format!("no mailbox {id}")))?);
    }
    match out.is_empty() {
        true => Err(invalid("an email is in at least one mailbox")),
        false => Ok(out),
    }
}

/// The flag bits `keywords` sets; a keyword this server cannot keep is
/// refused rather than dropped.
pub fn flags_of(keywords: Option<&Value>) -> Result<u32, NotFiled> {
    let Some(map) = keywords.and_then(|v| v.as_object()) else {
        return Ok(0);
    };
    let mut flags = 0;
    for (k, on) in map {
        if on.as_bool() != Some(true) {
            continue;
        }
        match mailrs_jmap::flags::keyword_to_flag(&k.to_ascii_lowercase()) {
            0 => return Err(invalid(format!("keyword {k} is not kept by this server"))),
            bit => flags |= bit,
        }
    }
    Ok(flags)
}

/// Every address in To, Cc and Bcc.
pub fn recipients(head: &mailrs_rfc5322::Message<'_>) -> Vec<String> {
    ["To", "Cc", "Bcc"]
        .iter()
        .flat_map(|h| head.header_all(h))
        .filter_map(|h| h.value_str())
        .flat_map(|v| {
            mailrs_rfc5322::split_list(v)
                .map(|a| mailrs_rfc5322::addr_spec(a).trim().to_string())
                .filter(|a| a.contains('@'))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Where a message being filed comes from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// `Email/import`: mail brought over, which stops at the quota like
    /// the mbox and IMAP imports, Sent and Drafts included.
    Imported,
    /// `Email/set` create: a message the user writes here, filed like a
    /// send.
    Written,
}

/// File `bytes` for the user into `places` with `flags`; the new uid.
pub async fn file(
    ctx: &Ctx,
    bytes: &[u8],
    places: &[Place],
    flags: u32,
    origin: Origin,
) -> Result<u32, NotFiled> {
    let core = &ctx.state.core;
    let own = places
        .iter()
        .any(|p| matches!(p, Place::Role(Role::Drafts | Role::Sent)));
    let filed = match (own, origin) {
        (true, Origin::Imported) => {
            let rcpts = recipients(&mailrs_rfc5322::Message::new(bytes));
            core.file_imported_sent(&ctx.user, bytes, &rcpts).await
        }
        (true, Origin::Written) => {
            let rcpts = recipients(&mailrs_rfc5322::Message::new(bytes));
            core.file_sent(&ctx.user, bytes, &rcpts).await
        }
        (false, _) => core.import_message(&ctx.user, bytes, None).await,
    };
    let filed = filed.map_err(not_filed)?;
    let w = core
        .find_by_message_id_for_user(&ctx.user, &filed.message_id)
        .await
        .map_err(|e| NotFiled("serverFail", e.to_string()))?;
    let mut want = flags & (FLAG_SEEN | FLAG_FLAGGED | mailrs_mailbox::types::FLAG_ANSWERED);
    if places.contains(&Place::Role(Role::Drafts)) {
        want |= FLAG_DRAFT;
    }
    // filed as sent mail, it is already read
    if own && flags & FLAG_SEEN == 0 && !places.contains(&Place::Role(Role::Drafts)) {
        want |= FLAG_SEEN;
    }
    if want != w.flags {
        let req = mailrs_core_api::method::admin::SetMessageFlagsRequest { flags: want };
        core.set_message_flags(&ctx.user, w.uid, &req)
            .await
            .map_err(|e| NotFiled("serverFail", e.to_string()))?;
    }
    if !own {
        email_set::move_thread(ctx, &filed.thread_id, None, places)
            .await
            .map_err(|e| NotFiled("serverFail", e.to_string()))?;
    }
    Ok(w.uid)
}

pub async fn import(
    ctx: &Ctx,
    args: &Value,
    state: &str,
) -> Result<(String, Value), JmapMethodError> {
    ctx.check_account(args)?;
    let emails = args
        .get("emails")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    if emails.len() > super::MAX_OBJECTS {
        return Err(super::limit_error("maxObjectsInSet"));
    }
    let (mut created, mut not_created) = (Map::new(), Map::new());
    for (cid, spec) in emails {
        match import_one(ctx, &spec).await {
            Ok((uid, size)) => {
                created.insert(cid, json!({ "id": ids::email(uid), "blobId": ids::blob(&Blob::Message(uid)), "size": size }));
            }
            Err(e) => {
                not_created.insert(cid, e.to_json());
            }
        }
    }
    Ok((
        "Email/import".into(),
        json!({
            "accountId": ctx.user,
            "oldState": state,
            "newState": state,
            "created": created,
            "notCreated": not_created,
        }),
    ))
}

async fn import_one(ctx: &Ctx, spec: &Value) -> Result<(u32, usize), NotFiled> {
    let Some(blob_id) = spec.get("blobId").and_then(|v| v.as_str()) else {
        return Err(invalid("blobId is required"));
    };
    let places = mailboxes_of(spec)?;
    let flags = flags_of(spec.get("keywords"))?;
    let bytes = blob::read(ctx, blob_id)
        .await
        .ok_or_else(|| NotFiled("blobNotFound", blob_id.to_string()))?;
    let uid = file(ctx, &bytes, &places, flags, Origin::Imported).await?;
    Ok((uid, bytes.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailboxes_and_keywords_are_read_strictly() {
        let spec = json!({ "mailboxIds": { "drafts": true, "L4": true, "inbox": false } });
        let p = mailboxes_of(&spec).ok().unwrap();
        assert_eq!(p.len(), 2);
        assert!(p.contains(&Place::Role(Role::Drafts)) && p.contains(&Place::Label("4".into())));
        assert!(mailboxes_of(&json!({ "mailboxIds": { "inbox": false } })).is_err());
        assert!(mailboxes_of(&json!({ "mailboxIds": { "nowhere": true } })).is_err());
        assert_eq!(
            flags_of(Some(&json!({ "$seen": true, "$flagged": false }))).ok(),
            Some(FLAG_SEEN)
        );
        assert!(flags_of(Some(&json!({ "$forwarded": true }))).is_err());
    }
}
