//! `Mailbox/set`: labels are made, renamed and deleted here; the roles
//! (inbox, archive, …) are places every account has and cannot change.

use mailrs_core_api::method::label::{CreateLabelRequest, LABEL_COLORS, UpdateLabelRequest};
use mailrs_jmap::error::JmapMethodError;
use serde_json::{Map, Value, json};

use super::ctx::Ctx;
use super::mailbox::Place;

fn set_error(kind: &str, description: &str) -> Value {
    json!({ "type": kind, "description": description })
}

fn name_of(spec: &Value) -> Option<String> {
    spec.get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
}

pub async fn set(ctx: &Ctx, args: &Value, state: &str) -> Result<(String, Value), JmapMethodError> {
    ctx.check_account(args)?;
    let core = &ctx.state.core;
    let (mut created, mut not_created) = (Map::new(), Map::new());
    if let Some(creates) = args.get("create").and_then(|v| v.as_object()) {
        for (cid, spec) in creates {
            if spec.get("parentId").is_some_and(|p| !p.is_null()) {
                not_created.insert(
                    cid.clone(),
                    set_error("invalidProperties", "mailboxes do not nest"),
                );
                continue;
            }
            let Some(name) = name_of(spec) else {
                not_created.insert(
                    cid.clone(),
                    set_error("invalidProperties", "name is required"),
                );
                continue;
            };
            let req = CreateLabelRequest {
                name,
                color: LABEL_COLORS[0].to_string(),
            };
            match core.create_label(&ctx.user, &req).await {
                Ok(l) => {
                    created.insert(cid.clone(), json!({ "id": Place::Label(l.id).id() }));
                }
                Err(e) => {
                    not_created.insert(cid.clone(), set_error("invalidProperties", &e.to_string()));
                }
            }
        }
    }
    let (mut updated, mut not_updated) = (Map::new(), Map::new());
    if let Some(updates) = args.get("update").and_then(|v| v.as_object()) {
        for (id, patch) in updates {
            let Some(Place::Label(label)) = Place::parse(id) else {
                not_updated.insert(
                    id.clone(),
                    set_error("forbidden", "this mailbox cannot be changed"),
                );
                continue;
            };
            let req = UpdateLabelRequest {
                name: name_of(patch),
                color: None,
            };
            match core.update_label(&ctx.user, &label, &req).await {
                Ok(_) => {
                    updated.insert(id.clone(), Value::Null);
                }
                Err(_) => {
                    not_updated.insert(id.clone(), set_error("notFound", "no such mailbox"));
                }
            }
        }
    }
    let (mut destroyed, mut not_destroyed) = (Vec::new(), Map::new());
    if let Some(ids) = args.get("destroy").and_then(|v| v.as_array()) {
        for id in ids.iter().filter_map(|v| v.as_str()) {
            let Some(Place::Label(label)) = Place::parse(id) else {
                not_destroyed.insert(
                    id.to_string(),
                    set_error("forbidden", "this mailbox cannot be deleted"),
                );
                continue;
            };
            match core.delete_label(&ctx.user, &label).await {
                Ok(()) => destroyed.push(id.to_string()),
                Err(_) => {
                    not_destroyed.insert(id.to_string(), set_error("notFound", "no such mailbox"));
                }
            }
        }
    }
    Ok((
        "Mailbox/set".into(),
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
