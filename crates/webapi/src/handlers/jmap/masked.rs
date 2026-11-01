//! JMAP `MaskedEmail/get` and `MaskedEmail/set`, Fastmail's extension that
//! password managers speak (capability [`MASKED_CAP`]). The operations are
//! the REST ones in `masked_email`; this is only the wire shape.
//!
//! `emailPrefix` on create is accepted and ignored: the address is drawn
//! as usual and returned, since a mask's words come from the reviewed list
//! and no caller may choose one that reads like an official address of
//! the domain. Refusing it would fail the password managers that send it.

use mailrs_core_sidestate::families::masked_email::{Mask, NewMask, Patch, State as MaskState};
use mailrs_jmap::error::JmapMethodError;
use serde_json::{Map, Value, json};

use crate::WebState;
use crate::handlers::masked_email::{self as ops, MaskError};

/// Fastmail's masked-email capability URI.
pub const MASKED_CAP: &str = "https://www.fastmail.com/dev/maskedemail";

fn utc(epoch: i64) -> Value {
    chrono::DateTime::from_timestamp(epoch, 0)
        .map(|t| Value::String(t.format("%Y-%m-%dT%H:%M:%SZ").to_string()))
        .unwrap_or(Value::Null)
}

fn state_name(s: MaskState) -> &'static str {
    match s {
        MaskState::Pending => "pending",
        MaskState::Enabled => "enabled",
        MaskState::Disabled => "disabled",
        MaskState::Deleted => "deleted",
    }
}

/// A mask as a `MaskedEmail` object.
fn object(m: &Mask) -> Value {
    json!({
        "id": m.id,
        "email": m.email,
        "state": state_name(m.state),
        "forDomain": m.for_domain,
        "description": m.description,
        "url": m.url,
        "createdAt": utc(m.created_at),
        "createdBy": m.created_by,
        "lastMessageAt": m.last_message_at.map(utc).unwrap_or(Value::Null),
    })
}

fn set_error(e: &MaskError) -> Value {
    let kind = match e {
        MaskError::NotFound => "notFound",
        MaskError::TooMany | MaskError::TooManyToday => "overQuota",
        MaskError::Domain => "forbidden",
        MaskError::Deleted | MaskError::Invalid(_) => "invalidProperties",
        MaskError::Unavailable => "serverFail",
    };
    json!({ "type": kind, "description": e.describe() })
}

fn unavailable(e: MaskError) -> JmapMethodError {
    JmapMethodError::ServerUnavailable(e.describe().to_string())
}

fn string(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Run one `MaskedEmail/*` call for `user`; `None` for any other method.
pub(crate) async fn dispatch(
    state: &WebState,
    user: &str,
    created_by: &str,
    method: &str,
    args: &Value,
) -> Option<Result<(String, Value), JmapMethodError>> {
    match method {
        "MaskedEmail/get" => Some(get(user, args)),
        "MaskedEmail/set" => Some(set(state, user, created_by, args).await),
        _ => None,
    }
}

fn get(user: &str, args: &Value) -> Result<(String, Value), JmapMethodError> {
    let masks = ops::list(user).map_err(unavailable)?;
    let (list, not_found): (Vec<Value>, Vec<Value>) = match args.get("ids") {
        Some(Value::Array(ids)) => {
            let mut found = Vec::new();
            let mut missing = Vec::new();
            for id in ids {
                match masks.iter().find(|m| Some(m.id.as_str()) == id.as_str()) {
                    Some(m) => found.push(object(m)),
                    None => missing.push(id.clone()),
                }
            }
            (found, missing)
        }
        _ => (masks.iter().map(object).collect(), Vec::new()),
    };
    Ok((
        "MaskedEmail/get".into(),
        json!({ "accountId": user, "state": "0", "list": list, "notFound": not_found }),
    ))
}

/// The address behind a `MaskedEmail` id among `user`'s live masks.
fn email_of(masks: &[Mask], id: &str) -> Option<String> {
    masks.iter().find(|m| m.id == id).map(|m| m.email.clone())
}

fn patch_of(v: &Value) -> Result<Patch, MaskError> {
    let state = string(v, "state")
        .map(|s| ops::settable_state(&s))
        .transpose()?;
    Ok(Patch {
        state,
        for_domain: string(v, "forDomain"),
        description: string(v, "description"),
        url: v.get("url").map(|u| u.as_str().map(str::to_string)),
    })
}

async fn set(
    state: &WebState,
    user: &str,
    created_by: &str,
    args: &Value,
) -> Result<(String, Value), JmapMethodError> {
    let mut created = Map::new();
    let mut not_created = Map::new();
    if let Some(Value::Object(creates)) = args.get("create") {
        for (cid, spec) in creates {
            let initial = match string(spec, "state").as_deref() {
                Some("pending") => Some(MaskState::Pending),
                Some("enabled") | None => Some(MaskState::Enabled),
                Some(_) => {
                    not_created
                        .insert(cid.clone(), set_error(&MaskError::Invalid("unknown state")));
                    continue;
                }
            };
            let new = NewMask {
                for_domain: string(spec, "forDomain").unwrap_or_default(),
                description: string(spec, "description").unwrap_or_default(),
                url: string(spec, "url"),
                created_by: created_by.to_string(),
                state: initial,
            };
            match ops::create(state, user, None, new).await {
                Ok(m) => {
                    created.insert(cid.clone(), object(&m));
                }
                Err(e) => {
                    not_created.insert(cid.clone(), set_error(&e));
                }
            }
        }
    }
    let masks = ops::list(user).map_err(unavailable)?;
    let mut updated = Map::new();
    let mut not_updated = Map::new();
    if let Some(Value::Object(updates)) = args.get("update") {
        for (id, spec) in updates {
            let outcome = email_of(&masks, id)
                .ok_or(MaskError::NotFound)
                .and_then(|email| patch_of(spec).map(|p| (email, p)))
                .and_then(|(email, p)| ops::update(user, &email, p));
            match outcome {
                Ok(_) => {
                    updated.insert(id.clone(), Value::Null);
                }
                Err(e) => {
                    not_updated.insert(id.clone(), set_error(&e));
                }
            }
        }
    }
    let mut destroyed = Vec::new();
    let mut not_destroyed = Map::new();
    if let Some(Value::Array(ids)) = args.get("destroy") {
        for id in ids.iter().filter_map(Value::as_str) {
            let gone = Patch {
                state: Some(MaskState::Deleted),
                ..Patch::default()
            };
            let outcome = email_of(&masks, id)
                .ok_or(MaskError::NotFound)
                .and_then(|email| ops::update(user, &email, gone));
            match outcome {
                Ok(_) => destroyed.push(Value::String(id.to_string())),
                Err(e) => {
                    not_destroyed.insert(id.to_string(), set_error(&e));
                }
            }
        }
    }
    Ok((
        "MaskedEmail/set".into(),
        json!({
            "accountId": user,
            "oldState": "0",
            "newState": "0",
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
    use super::*;

    fn mask(state: MaskState) -> Mask {
        Mask {
            id: "me-amber.otter.0042".into(),
            email: "amber.otter.000042@x.jp".into(),
            owner: "me@x.jp".into(),
            state,
            for_domain: "https://shop.example".into(),
            description: "shop".into(),
            url: None,
            created_at: 1_700_000_000,
            created_by: "jmap".into(),
            last_message_at: None,
        }
    }

    #[test]
    fn a_mask_reads_as_fastmail_writes_it() {
        let o = object(&mask(MaskState::Enabled));
        assert_eq!(o["state"], "enabled");
        assert_eq!(o["forDomain"], "https://shop.example");
        assert_eq!(o["createdAt"], "2023-11-14T22:13:20Z");
        assert_eq!(o["lastMessageAt"], Value::Null);
        assert_eq!(o["email"], "amber.otter.000042@x.jp");
    }

    #[test]
    fn an_update_takes_known_fields_and_refuses_pending() {
        let p = patch_of(&json!({ "state": "disabled", "url": null })).unwrap();
        assert_eq!(p.state, Some(MaskState::Disabled));
        assert_eq!(p.url, Some(None));
        assert!(patch_of(&json!({ "state": "pending" })).is_err());
        assert!(patch_of(&json!({ "state": "gone" })).is_err());
    }
}
