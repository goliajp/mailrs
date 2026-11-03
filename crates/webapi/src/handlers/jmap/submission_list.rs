//! `EmailSubmission/get`, `/changes`, and the update and destroy half of
//! `/set` (RFC 8621 §7): each submission is a send the Send list records,
//! whichever client made it.
//!
//! A submission's id is `S` + a 64-bit hash of its send id (the sent
//! message's Message-ID, which carries characters an Id may not). Its
//! state is kept the way an Email state is (see `state`): a snapshot of
//! (id, version) pairs in the network kevy, under the account name with
//! `/submission` appended, which no address contains.
//!
//! Undo is a cancel through the same path as the web's: a send that is
//! still held (`pending`) can be stopped; once it is going out it is
//! `final`, and asking to cancel it answers `cannotUnsend`.

use mailrs_core_api::method::message_index::fnv64;
use mailrs_core_sidestate::families::send as sendfam;
use mailrs_core_sidestate::families::send::Status;
use mailrs_core_sidestate::families::send_read::{self, SendListItem};
use mailrs_jmap::error::JmapMethodError;
use serde_json::{Map, Value, json};

use super::ctx::{Ctx, fail, requested_ids};
use super::state::{self, Snapshot};
use super::{identity, ids};
use crate::handlers::kevy_util::with_kevy;

/// The most submissions an account lists: its newest sends.
const MAX_LISTED: i64 = 2_000;
const PAGE: i64 = 200;

pub fn submission_id(send_id: &str) -> String {
    format!("S{:016x}", fnv64(send_id.as_bytes()))
}

pub struct Sub {
    pub item: SendListItem,
    pub send_at: Option<i64>,
}

impl Sub {
    pub fn id(&self) -> String {
        submission_id(&self.item.send_id)
    }

    fn undo_status(&self) -> &'static str {
        match self.item.status {
            Status::Scheduled => "pending",
            Status::Cancelled => "canceled",
            _ => "final",
        }
    }

    /// What a client sees change: the status, when it goes, and each
    /// recipient's outcome.
    fn version(&self) -> u32 {
        let mut b = self.item.status.as_str().as_bytes().to_vec();
        b.extend_from_slice(&self.send_at.unwrap_or(0).to_le_bytes());
        for r in &self.item.recipients {
            b.extend_from_slice(r.recipient.as_bytes());
            b.push(u8::from(r.delivered) | (u8::from(r.pending) << 1));
            b.extend_from_slice(&r.code.to_le_bytes());
        }
        fnv64(&b) as u32
    }
}

/// The account's sends, newest first.
pub fn all(user: &str) -> Result<Vec<Sub>, JmapMethodError> {
    let user = user.to_string();
    with_kevy(move |c| {
        let mut out = Vec::new();
        let mut offset = 0;
        while offset < MAX_LISTED {
            let page = send_read::list_sends(c, &user, None, offset, PAGE)?;
            let n = page.len() as i64;
            for item in page {
                let send_at = sendfam::send_at(c, &user, &item.send_id)?;
                out.push(Sub { item, send_at });
            }
            if n < PAGE {
                break;
            }
            offset += n;
        }
        Ok(out)
    })
    .map_err(|_| fail("send records unavailable"))
}

fn namespace(user: &str) -> String {
    format!("{user}/submission")
}

/// The state naming `subs`, kept for a later `/changes`.
pub fn state_of(user: &str, subs: &[Sub]) -> Result<String, JmapMethodError> {
    let mut pairs: Vec<(String, u32)> = subs.iter().map(|s| (s.id(), s.version())).collect();
    pairs.sort();
    let snap = Snapshot {
        emails: Vec::new(),
        mailboxes: pairs,
    };
    let st = snap.state();
    state::remember(&namespace(user), &st, &snap)?;
    Ok(st)
}

fn rfc3339(epoch: i64) -> Value {
    chrono::DateTime::from_timestamp(epoch.max(0), 0)
        .map(|t| json!(t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)))
        .unwrap_or(Value::Null)
}

/// The sending address, from the sent copy; the account's own when the
/// copy cannot be read.
async fn from_of(ctx: &Ctx, s: &Sub) -> String {
    let raw = match s.item.can_resend() {
        true => crate::handlers::messages::read_maildir_bytes_pub(&ctx.user, &s.item.envelope_ref)
            .await
            .ok(),
        false => None,
    };
    raw.and_then(|r| {
        mailrs_rfc5322::Message::new(&r)
            .header_str("From")
            .map(|f| mailrs_rfc5322::addr_spec(f).trim().to_ascii_lowercase())
    })
    .unwrap_or_else(|| ctx.user.to_ascii_lowercase())
}

async fn object(ctx: &Ctx, s: &Sub) -> Value {
    let from = from_of(ctx, s).await;
    let message_id = s.item.send_id.split('#').next().unwrap_or_default();
    let email_id = ctx
        .state
        .core
        .find_by_message_id_for_user(&ctx.user, message_id)
        .await
        .ok()
        .map(|w| ids::email(w.uid));
    let mut delivery = Map::new();
    for r in &s.item.recipients {
        let delivered = match (r.delivered, r.pending) {
            (true, _) => "yes",
            (false, true) => "queued",
            (false, false) => "no",
        };
        let reply = match r.code {
            0 => String::new(),
            code => format!("{code} {}", r.message),
        };
        delivery.insert(
            r.recipient.clone(),
            json!({ "smtpReply": reply, "delivered": delivered, "displayed": "unknown" }),
        );
    }
    let rcpt: Vec<Value> = s
        .item
        .recipients
        .iter()
        .map(|r| json!({ "email": r.recipient, "parameters": null }))
        .collect();
    json!({
        "id": s.id(),
        "identityId": identity::id(&from),
        "emailId": email_id,
        "threadId": ids::thread(ids::thread_key(&s.item.thread_id)),
        "envelope": { "mailFrom": { "email": from, "parameters": null }, "rcptTo": rcpt },
        "sendAt": rfc3339(s.send_at.unwrap_or(s.item.created_at)),
        "undoStatus": s.undo_status(),
        "deliveryStatus": delivery,
        "dsnBlobIds": [],
        "mdnBlobIds": [],
    })
}

fn only(o: Value, props: Option<&Vec<Value>>) -> Value {
    let Some(props) = props else {
        return o;
    };
    let mut out = Map::new();
    out.insert("id".into(), o["id"].clone());
    for p in props.iter().filter_map(|p| p.as_str()) {
        if let Some(v) = o.get(p) {
            out.insert(p.to_string(), v.clone());
        }
    }
    Value::Object(out)
}

pub async fn get(ctx: &Ctx, args: &Value) -> Result<(String, Value), JmapMethodError> {
    ctx.check_account(args)?;
    let subs = all(&ctx.user)?;
    let st = state_of(&ctx.user, &subs)?;
    let wanted = requested_ids(args);
    if wanted
        .as_ref()
        .is_some_and(|w| w.len() > super::MAX_OBJECTS)
    {
        return Err(super::limit_error("maxObjectsInGet"));
    }
    let props = args.get("properties").and_then(|v| v.as_array());
    let mut list = Vec::new();
    let mut not_found = Vec::new();
    match &wanted {
        None => {
            for s in subs.iter().take(super::MAX_OBJECTS) {
                list.push(only(object(ctx, s).await, props));
            }
        }
        Some(w) => {
            for id in w {
                match subs.iter().find(|s| s.id() == *id) {
                    Some(s) => list.push(only(object(ctx, s).await, props)),
                    None => not_found.push(id.clone()),
                }
            }
        }
    }
    Ok((
        "EmailSubmission/get".into(),
        json!({ "accountId": ctx.user, "state": st, "list": list, "notFound": not_found }),
    ))
}

pub async fn changes(ctx: &Ctx, args: &Value) -> Result<(String, Value), JmapMethodError> {
    ctx.check_account(args)?;
    let Some(since) = args.get("sinceState").and_then(|v| v.as_str()) else {
        return Err(JmapMethodError::InvalidArguments("sinceState".into()));
    };
    let old = state::recall(&namespace(&ctx.user), since)?.ok_or_else(state::cannot_calculate)?;
    let subs = all(&ctx.user)?;
    let st = state_of(&ctx.user, &subs)?;
    let mut pairs: Vec<(String, u32)> = subs.iter().map(|s| (s.id(), s.version())).collect();
    pairs.sort();
    let now = Snapshot {
        emails: Vec::new(),
        mailboxes: pairs,
    };
    let d = state::mailboxes(&old, &now);
    let max = args.get("maxChanges").and_then(|v| v.as_u64());
    if max.is_some_and(|m| d.len() as u64 > m) {
        return Err(state::cannot_calculate());
    }
    Ok((
        "EmailSubmission/changes".into(),
        json!({
            "accountId": ctx.user,
            "oldState": since,
            "newState": st,
            "hasMoreChanges": false,
            "created": d.created,
            "updated": d.updated,
            "destroyed": d.destroyed,
        }),
    ))
}

/// The update and destroy half of `/set`: `updated`, `notUpdated`,
/// `notDestroyed`.
pub fn change(
    ctx: &Ctx,
    subs: &[Sub],
    args: &Value,
) -> (Map<String, Value>, Map<String, Value>, Map<String, Value>) {
    let (mut updated, mut not_updated, mut not_destroyed) = (Map::new(), Map::new(), Map::new());
    if let Some(u) = args.get("update").and_then(|v| v.as_object()) {
        for (id, patch) in u {
            let result = match subs.iter().find(|s| s.id() == *id) {
                None => Err(json!({ "type": "notFound" })),
                Some(s) => undo(ctx, s, patch),
            };
            match result {
                Ok(()) => {
                    updated.insert(id.clone(), Value::Null);
                }
                Err(e) => {
                    not_updated.insert(id.clone(), e);
                }
            }
        }
    }
    if let Some(d) = args.get("destroy").and_then(|v| v.as_array()) {
        for id in d.iter().filter_map(|v| v.as_str()) {
            let e = match subs.iter().any(|s| s.id() == id) {
                true => {
                    json!({ "type": "forbidden", "description": "the record of a send is kept; cancel one with undoStatus" })
                }
                false => json!({ "type": "notFound" }),
            };
            not_destroyed.insert(id.to_string(), e);
        }
    }
    (updated, not_updated, not_destroyed)
}

/// The one change a submission takes: `undoStatus` to `canceled`.
fn undo(ctx: &Ctx, s: &Sub, patch: &Value) -> Result<(), Value> {
    let Some(p) = patch.as_object() else {
        return Err(json!({ "type": "invalidPatch" }));
    };
    let asks_cancel =
        p.len() == 1 && p.get("undoStatus").and_then(|v| v.as_str()) == Some("canceled");
    if !asks_cancel {
        return Err(
            json!({ "type": "invalidProperties", "description": "only undoStatus: canceled can be set" }),
        );
    }
    if s.item.status != Status::Scheduled {
        return Err(json!({ "type": "cannotUnsend", "description": "it is no longer held" }));
    }
    match crate::handlers::sends::cancel(&ctx.user, &s.item.send_id) {
        Ok(r) if r.already_delivered == 0 => Ok(()),
        Ok(_) | Err(axum::http::StatusCode::CONFLICT) => Err(
            json!({ "type": "cannotUnsend", "description": "it has already gone to a recipient" }),
        ),
        Err(e) => Err(json!({ "type": "serverFail", "description": e.to_string() })),
    }
}
