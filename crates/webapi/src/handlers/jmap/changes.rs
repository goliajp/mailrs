//! `Mailbox/changes`, `Email/changes`, `Thread/changes` and
//! `Email/queryChanges`: the difference between the snapshot the client's
//! state names and the one now (see `state`).

use mailrs_jmap::error::JmapMethodError;
use serde_json::{Value, json};

use super::ctx::Ctx;
use super::email_query;
use super::ids;
use super::state::{self, Diff, Now, Snapshot};
use super::view::View;

pub fn cannot() -> Result<(String, Value), JmapMethodError> {
    Err(state::cannot_calculate())
}

/// The snapshot `sinceState` names, and how many changes the client takes.
fn since(ctx: &Ctx, args: &Value) -> Result<(Snapshot, Option<usize>), JmapMethodError> {
    ctx.check_account(args)?;
    let Some(since) = args.get("sinceState").and_then(|v| v.as_str()) else {
        return Err(JmapMethodError::InvalidArguments("sinceState".into()));
    };
    let max = match args.get("maxChanges") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_u64() {
            Some(n) if n > 0 => Some(n as usize),
            _ => return Err(JmapMethodError::InvalidArguments("maxChanges".into())),
        },
    };
    let old = state::recall(&ctx.user, since)?.ok_or_else(state::cannot_calculate)?;
    Ok((old, max))
}

/// The answer for one type. A difference larger than `maxChanges` cannot
/// be cut into steps (there is no state in between to name), so the
/// client is told to sync again.
fn answer<K>(
    ctx: &Ctx,
    method: &str,
    old_state: &str,
    now: &Now,
    d: Diff<K>,
    max: Option<usize>,
    id: impl Fn(&K) -> String,
) -> Result<(String, Value), JmapMethodError> {
    if max.is_some_and(|m| d.len() > m) {
        return Err(state::cannot_calculate());
    }
    let ids = |list: &[K]| list.iter().map(&id).collect::<Vec<_>>();
    Ok((
        method.to_string(),
        json!({
            "accountId": ctx.user,
            "oldState": old_state,
            "newState": now.state,
            "hasMoreChanges": false,
            "created": ids(&d.created),
            "updated": ids(&d.updated),
            "destroyed": ids(&d.destroyed),
        }),
    ))
}

fn since_state(args: &Value) -> &str {
    args.get("sinceState")
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

pub fn email(ctx: &Ctx, now: &Now, args: &Value) -> Result<(String, Value), JmapMethodError> {
    let (old, max) = since(ctx, args)?;
    let d = state::emails(&old, &now.snapshot);
    answer(ctx, "Email/changes", since_state(args), now, d, max, |u| {
        ids::email(*u)
    })
}

pub fn thread(ctx: &Ctx, now: &Now, args: &Value) -> Result<(String, Value), JmapMethodError> {
    let (old, max) = since(ctx, args)?;
    let d = state::threads(&old, &now.snapshot);
    answer(ctx, "Thread/changes", since_state(args), now, d, max, |t| {
        ids::thread(*t)
    })
}

pub fn mailbox(ctx: &Ctx, now: &Now, args: &Value) -> Result<(String, Value), JmapMethodError> {
    let (old, max) = since(ctx, args)?;
    let d = state::mailboxes(&old, &now.snapshot);
    let (_, mut v) = answer(
        ctx,
        "Mailbox/changes",
        since_state(args),
        now,
        d,
        max,
        String::clone,
    )?;
    v["updatedProperties"] = Value::Null;
    Ok(("Mailbox/changes".into(), v))
}

/// Every message that was created, changed or destroyed is reported
/// removed, and those of them in the results now are added back at their
/// place, which RFC 8620 §5.6 allows; the rest kept their places.
pub async fn email_query(
    ctx: &Ctx,
    view: &View,
    now: &Now,
    args: &Value,
) -> Result<(String, Value), JmapMethodError> {
    ctx.check_account(args)?;
    let Some(since) = args.get("sinceQueryState").and_then(|v| v.as_str()) else {
        return Err(JmapMethodError::InvalidArguments("sinceQueryState".into()));
    };
    let old = state::recall(&ctx.user, since)?.ok_or_else(state::cannot_calculate)?;
    let q = email_query::parse(args)?;
    let touched = state::touched(&state::emails(&old, &now.snapshot));
    let max = args
        .get("maxChanges")
        .and_then(|v| v.as_u64())
        .map(|m| m as usize);
    if max.is_some_and(|m| touched.len() * 2 > m) {
        return Err(state::cannot_calculate());
    }
    let up_to = args
        .get("upToId")
        .and_then(|v| v.as_str())
        .and_then(ids::parse_email);
    let results = email_query::results(ctx, view, &q, usize::MAX).await?;
    let mut added = Vec::new();
    for (index, uid) in results.iter().enumerate() {
        if touched.contains(uid) {
            added.push(json!({ "id": ids::email(*uid), "index": index }));
        }
        if Some(*uid) == up_to {
            break;
        }
    }
    let removed: Vec<String> = touched.iter().map(|u| ids::email(*u)).collect();
    let mut out = json!({
        "accountId": ctx.user,
        "oldQueryState": since,
        "newQueryState": now.state,
        "removed": removed,
        "added": added,
    });
    if args.get("calculateTotal").and_then(|v| v.as_bool()) == Some(true) {
        out["total"] = json!(results.len());
    }
    Ok(("Email/queryChanges".into(), out))
}
