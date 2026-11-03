//! `VacationResponse/get` and `/set` (RFC 8621 §8): the vacation reply the
//! web's settings page edits, the same stored record.
//!
//! The reply is plain text, so `htmlBody` is always null and setting it is
//! refused. How often a sender is answered (`days`) is not a JMAP property
//! and is left as it was.

use mailrs_core_sidestate::families::vacation::{self, Vacation};
use mailrs_jmap::error::JmapMethodError;
use serde_json::{Map, Value, json};

use super::ctx::{Ctx, fail, requested_ids};
use crate::handlers::kevy_util::with_kevy;

pub const VACATION_CAP: &str = "urn:ietf:params:jmap:vacationresponse";
const SINGLETON: &str = "singleton";

fn stored(user: &str) -> Result<Vacation, JmapMethodError> {
    let u = user.to_string();
    let v = with_kevy(move |c| vacation::get(c, &u)).map_err(|_| fail("vacation store"))?;
    Ok(v.unwrap_or(Vacation {
        enabled: false,
        subject: String::new(),
        body: String::new(),
        start: None,
        end: None,
        days: 7,
        version: 0,
    }))
}

fn date(epoch: Option<i64>) -> Value {
    epoch
        .and_then(|e| chrono::DateTime::from_timestamp(e, 0))
        .map(|t| json!(t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)))
        .unwrap_or(Value::Null)
}

fn object(v: &Vacation) -> Value {
    let text = |s: &str| match s.is_empty() {
        true => Value::Null,
        false => json!(s),
    };
    json!({
        "id": SINGLETON,
        "isEnabled": v.enabled,
        "fromDate": date(v.start),
        "toDate": date(v.end),
        "subject": text(&v.subject),
        "textBody": text(&v.body),
        "htmlBody": null,
    })
}

fn state_of(v: &Vacation) -> String {
    v.version.to_string()
}

pub fn get(ctx: &Ctx, args: &Value) -> Result<(String, Value), JmapMethodError> {
    ctx.check_account(args)?;
    let v = stored(&ctx.user)?;
    let (mut list, mut not_found) = (Vec::new(), Vec::new());
    match requested_ids(args) {
        None => list.push(object(&v)),
        Some(ids) => {
            for id in ids {
                match id == SINGLETON {
                    true => list.push(object(&v)),
                    false => not_found.push(id),
                }
            }
        }
    }
    Ok((
        "VacationResponse/get".into(),
        json!({ "accountId": ctx.user, "state": state_of(&v), "list": list, "notFound": not_found }),
    ))
}

fn invalid(why: &str) -> Value {
    json!({ "type": "invalidProperties", "description": why })
}

/// `UTCDate` or null into epoch seconds; `Err` for anything else.
fn epoch(v: &Value) -> Result<Option<i64>, ()> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) => chrono::DateTime::parse_from_rfc3339(s)
            .map(|t| Some(t.timestamp()))
            .map_err(|_| ()),
        _ => Err(()),
    }
}

/// `patch` applied to `v`, or why not.
fn apply(mut v: Vacation, patch: &Map<String, Value>) -> Result<Vacation, Value> {
    for (k, val) in patch {
        match k.as_str() {
            "id" if val == SINGLETON => {}
            "isEnabled" => v.enabled = val.as_bool().ok_or_else(|| invalid("isEnabled"))?,
            "fromDate" => v.start = epoch(val).map_err(|_| invalid("fromDate"))?,
            "toDate" => v.end = epoch(val).map_err(|_| invalid("toDate"))?,
            "subject" | "textBody" => {
                let s = match val {
                    Value::Null => String::new(),
                    Value::String(s) => s.clone(),
                    _ => return Err(invalid(k)),
                };
                match k.as_str() {
                    "subject" => v.subject = s,
                    _ => v.body = s,
                }
            }
            "htmlBody" if val.is_null() => {}
            "htmlBody" => return Err(invalid("htmlBody: the reply is plain text; set textBody")),
            other => return Err(invalid(other)),
        }
    }
    Ok(v)
}

pub fn set(ctx: &Ctx, args: &Value) -> Result<(String, Value), JmapMethodError> {
    ctx.check_account(args)?;
    let before = stored(&ctx.user)?;
    let old_state = state_of(&before);
    if let Some(want) = args.get("ifInState").and_then(|v| v.as_str())
        && want != old_state
    {
        return Err(JmapMethodError::Other {
            kind: "stateMismatch".into(),
            description: String::new(),
        });
    }
    let singleton = || json!({ "type": "singleton" });
    let mut not_created = Map::new();
    if let Some(c) = args.get("create").and_then(|v| v.as_object()) {
        for k in c.keys() {
            not_created.insert(k.clone(), singleton());
        }
    }
    let mut not_destroyed = Map::new();
    if let Some(d) = args.get("destroy").and_then(|v| v.as_array()) {
        for id in d.iter().filter_map(|v| v.as_str()) {
            not_destroyed.insert(id.to_string(), singleton());
        }
    }
    let (mut updated, mut not_updated) = (Map::new(), Map::new());
    let mut now = before.clone();
    if let Some(u) = args.get("update").and_then(|v| v.as_object()) {
        for (id, patch) in u {
            if id != SINGLETON {
                not_updated.insert(id.clone(), json!({ "type": "notFound" }));
                continue;
            }
            let Some(patch) = patch.as_object() else {
                not_updated.insert(id.clone(), invalid("patch"));
                continue;
            };
            let next = match apply(now.clone(), patch) {
                Ok(n) => n,
                Err(e) => {
                    not_updated.insert(id.clone(), e);
                    continue;
                }
            };
            let user = ctx.user.clone();
            let saved = with_kevy(move |c| vacation::set(c, &user, next))
                .map_err(|_| fail("vacation store"))?;
            match saved {
                Ok(s) => {
                    now = s;
                    updated.insert(id.clone(), Value::Null);
                }
                Err(why) => {
                    not_updated.insert(id.clone(), invalid(why.code()));
                }
            }
        }
    }
    Ok((
        "VacationResponse/set".into(),
        json!({
            "accountId": ctx.user,
            "oldState": old_state,
            "newState": state_of(&now),
            "created": {},
            "updated": updated,
            "destroyed": [],
            "notCreated": not_created,
            "notUpdated": not_updated,
            "notDestroyed": not_destroyed,
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn away() -> Vacation {
        Vacation {
            enabled: false,
            subject: String::new(),
            body: String::new(),
            start: None,
            end: None,
            days: 7,
            version: 3,
        }
    }

    #[test]
    fn a_patch_reaches_the_stored_fields_and_html_is_refused() {
        let p = json!({
            "isEnabled": true, "subject": "Away", "textBody": "Back Monday",
            "fromDate": "2026-11-10T00:00:00Z", "toDate": null,
        });
        let v = apply(away(), p.as_object().unwrap()).expect("applied");
        assert!(v.enabled);
        assert_eq!(
            (v.subject.as_str(), v.body.as_str()),
            ("Away", "Back Monday")
        );
        assert_eq!(v.start, Some(1_794_268_800));
        assert_eq!(v.end, None);
        assert_eq!(v.days, 7, "days is not a JMAP property and is kept");
        let o = object(&v);
        assert_eq!(o["fromDate"], "2026-11-10T00:00:00Z");
        assert_eq!(o["htmlBody"], Value::Null);
        assert!(
            apply(
                away(),
                json!({ "htmlBody": "<p>x</p>" }).as_object().unwrap()
            )
            .is_err()
        );
        assert!(apply(away(), json!({ "fromDate": "monday" }).as_object().unwrap()).is_err());
        assert!(apply(away(), json!({ "days": 3 }).as_object().unwrap()).is_err());
    }
}
