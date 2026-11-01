//! `Thread/get`: the messages of a conversation, from the view.

use mailrs_jmap::error::JmapMethodError;
use serde_json::{Value, json};

use super::ctx::{Ctx, requested_ids};
use super::ids;
use super::view::View;

pub fn get(
    ctx: &Ctx,
    view: &View,
    args: &Value,
    state: &str,
) -> Result<(String, Value), JmapMethodError> {
    ctx.check_account(args)?;
    let Some(wanted) = requested_ids(args) else {
        return Err(JmapMethodError::Other {
            kind: "requestTooLarge".into(),
            description: "Thread/get needs ids".into(),
        });
    };
    if wanted.len() > super::MAX_OBJECTS {
        return Err(super::limit_error("maxObjectsInGet"));
    }
    let mut list = Vec::new();
    let mut not_found = Vec::new();
    for id in wanted {
        let members = ids::parse_thread(&id)
            .map(|key| view.thread_members(key))
            .unwrap_or_default();
        match members.is_empty() {
            true => not_found.push(id),
            false => list.push(json!({
                "id": id,
                "emailIds": members.into_iter().map(ids::email).collect::<Vec<_>>(),
            })),
        }
    }
    Ok((
        "Thread/get".into(),
        json!({ "accountId": ctx.user, "state": state, "list": list, "notFound": not_found }),
    ))
}
